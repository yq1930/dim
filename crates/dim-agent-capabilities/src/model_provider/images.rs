//! Image blocks in a request the model cannot take become text, per request
//! (Attachments A.1). The rule covers user content and tool results alike.
//! Tool-result images older than the newest few become text too: every later
//! request would carry them again. The adopted input and the history are never
//! changed.
use base64::Engine;
use dim_agent_types::*;
use std::sync::Arc;

const NO_IMAGE_INPUT: &str = "[Image omitted: the current model does not accept images.]";
const UNSENDABLE: &str = "[Image omitted: the image cannot be sent to this model.]";
const EVICTED: &str = "[Image removed from context: only the most recent tool images are kept. Run the tool again to see it.]";

/// The tool-result images a request carries, counting from the newest. One the
/// model has not answered yet is carried whatever the count.
const KEPT_TOOL_IMAGES: usize = 3;

/// Image formats every adapter accepts (feature list).
const FORMATS: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/// Returns the request to send: the same request when every image is sent as
/// is, otherwise a copy whose unsendable and evicted images are text
/// placeholders.
pub(super) fn adapt(request: &Arc<ModelRequest>) -> Arc<ModelRequest> {
    // Unknown capability: the image is sent and the provider decides (A4).
    let vision = request
        .capability_snapshot
        .as_ref()
        .is_none_or(|snapshot| snapshot.capabilities.vision);
    let mut evict = evicted(&request.messages, vision);
    let replaced: Vec<_> = request
        .messages
        .iter()
        .map(|message| replace_images(message, vision, &mut evict))
        .collect();
    if replaced.iter().all(Option::is_none) {
        return request.clone();
    }
    let mut adapted = (**request).clone();
    adapted.messages = request
        .messages
        .iter()
        .zip(replaced)
        .map(|(message, replaced)| replaced.map_or_else(|| message.clone(), Arc::new))
        .collect();
    Arc::new(adapted)
}

/// How many of the oldest sendable tool-result images leave the request: those
/// before the last assistant message, beyond [`KEPT_TOOL_IMAGES`].
fn evicted(messages: &[Arc<Message>], vision: bool) -> usize {
    let answered = messages
        .iter()
        .rposition(|message| message.role == Role::Assistant)
        .map_or(0, |index| index + 1);
    let count = |messages: &[Arc<Message>]| -> usize {
        messages
            .iter()
            .map(|message| tool_images(message, vision))
            .sum()
    };
    let pending = count(&messages[answered..]);
    count(&messages[..answered]).saturating_sub(KEPT_TOOL_IMAGES.saturating_sub(pending))
}

/// The sendable images in the message's tool results.
fn tool_images(message: &Message, vision: bool) -> usize {
    let MessageContent::Parts(parts) = &message.content else {
        return 0;
    };
    parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::ToolResult {
                content: ToolContent::Blocks(blocks),
                ..
            } => Some(blocks),
            _ => None,
        })
        .flatten()
        .filter(|block| {
            matches!(block, ContentBlock::Image { source } if placeholder(source, vision).is_none())
        })
        .count()
}

/// The message with its unsendable images replaced and its first `evict`
/// sendable tool-result images removed, or `None` if nothing changed.
fn replace_images(message: &Message, vision: bool, evict: &mut usize) -> Option<Message> {
    let MessageContent::Parts(parts) = &message.content else {
        return None;
    };
    let mut changed = false;
    let parts = parts
        .iter()
        .map(|part| match part {
            MessagePart::Image { source } => {
                let Some(text) = placeholder(source, vision) else {
                    return part.clone();
                };
                changed = true;
                MessagePart::Text {
                    provider_metadata: None,
                    text: text.into(),
                    start_time: None,
                    end_time: None,
                }
            }
            MessagePart::ToolResult {
                tool_use_id,
                content,
                structured_content,
                is_error,
            } => {
                let ToolContent::Blocks(blocks) = content else {
                    return part.clone();
                };
                let mut replaced = false;
                let blocks = blocks
                    .iter()
                    .map(|block| match block {
                        ContentBlock::Image { source } => {
                            let text = placeholder(source, vision).or_else(|| {
                                let evicted = *evict > 0;
                                *evict -= usize::from(evicted);
                                evicted.then_some(EVICTED)
                            });
                            match text {
                                Some(text) => {
                                    replaced = true;
                                    ContentBlock::Text { text: text.into() }
                                }
                                None => block.clone(),
                            }
                        }
                        block => block.clone(),
                    })
                    .collect();
                if !replaced {
                    return part.clone();
                }
                changed = true;
                MessagePart::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: ToolContent::Blocks(blocks),
                    structured_content: structured_content.clone(),
                    is_error: *is_error,
                }
            }
            _ => part.clone(),
        })
        .collect();
    changed.then(|| Message {
        role: message.role,
        content: MessageContent::Parts(parts),
    })
}

/// The placeholder an image becomes, or `None` when the image is sent as is.
fn placeholder(source: &ImageSource, vision: bool) -> Option<&'static str> {
    if !vision {
        Some(NO_IMAGE_INPUT)
    } else if !sendable(source) {
        Some(UNSENDABLE)
    } else {
        None
    }
}

/// A URL source, undecodable data or a format adapters do not accept (A10).
fn sendable(source: &ImageSource) -> bool {
    source.source_type == ImageSourceType::Base64
        && FORMATS.contains(&source.media_type.as_str())
        && source.data.as_deref().is_some_and(|data| {
            !data.is_empty()
                && base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .is_ok()
        })
}
