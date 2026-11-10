//! Window facts of a profile that compacts, from the model's declared limits
//! with a local token estimate (ContextCompaction §5.1).
use crate::host::*;
use dim_agent_types::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// The trigger leaves `RESERVE_RATIO` of the window: the next request's
/// output allowance plus headroom for its estimation error, `HEADROOM_TOKENS`
/// and at most half the reserve.
const RESERVE_RATIO: f64 = 0.169;
const HEADROOM_TOKENS: f64 = 16_000.;
/// The reserve never cuts the output allowance below this; a window too small
/// for it reserves more.
const OUTPUT_FLOOR_TOKENS: f64 = 8_192.;
/// Serialized UTF-8 bytes per estimated token.
const BYTES_PER_TOKEN: f64 = 4.;
/// Estimated tokens of one image part.
const IMAGE_TOKENS: f64 = 1_600.;

/// Resolves window facts from the request alone; no service is needed.
pub struct ProductContextModel;
impl ResolveContextModel for ProductContextModel {
    fn resolve(
        &self,
        request: Arc<ModelRequest>,
        _: CancellationToken,
    ) -> HostFuture<'_, Arc<dyn ContextModel>> {
        Box::pin(async move {
            let snapshot = request.capability_snapshot.as_ref().ok_or_else(|| {
                HostError::classified(
                    HostErrorKind::InvalidRequest,
                    "Model capabilities are not adopted; select the model again",
                )
            })?;
            snapshot
                .validate_for(&request.provider_id, &request.model_id)
                .map_err(|message| HostError::classified(HostErrorKind::InvalidRequest, message))?;
            Ok(window_facts(
                snapshot.capabilities.context_window,
                snapshot.capabilities.max_output_tokens,
                &request,
            ))
        })
    }
}

/// Window facts for a model whose limits are declared: `context_window` and
/// its `max_output_tokens` limit, which the request's own `maxTokens` never
/// exceeds. The output allowance is what fits the reserve beside its headroom.
pub fn window_facts(
    context_window: u64,
    max_output_tokens: u64,
    request: &ModelRequest,
) -> Arc<dyn ContextModel> {
    let window = context_window as f64;
    let limit = max_output_tokens as f64;
    let requested = request
        .config
        .as_ref()
        .and_then(|config| config.max_tokens)
        .map_or(limit, |max| max.min(limit));
    let reserve = (RESERVE_RATIO * window).round();
    let headroom = HEADROOM_TOKENS.min((reserve / 2.).floor());
    let output_tokens = requested.min((reserve - headroom).max(OUTPUT_FLOOR_TOKENS));
    Arc::new(WindowFacts {
        context_window: window,
        output_tokens,
        reserve: reserve.max(output_tokens + headroom),
        auto_compact_window: request
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.startup.as_ref())
            .and_then(|startup| startup.auto_compact_window),
    })
}

struct WindowFacts {
    context_window: f64,
    output_tokens: f64,
    reserve: f64,
    auto_compact_window: Option<u64>,
}
impl ContextModel for WindowFacts {
    fn context_window(&self) -> f64 {
        self.context_window
    }
    fn output_tokens(&self) -> f64 {
        self.output_tokens
    }
    fn threshold_tokens(&self) -> f64 {
        let threshold = (self.context_window - self.reserve).max(0.);
        self.auto_compact_window.map_or(threshold, |window| {
            threshold.min((window as f64 - self.output_tokens).max(0.))
        })
    }
    fn count_tokens(&self, request: &ModelRequest) -> f64 {
        let mut bytes = size(&request.system) + size(&request.tools);
        let mut images = 0.;
        for message in &request.messages {
            match &message.content {
                MessageContent::Text(_) => bytes += size(message.as_ref()),
                MessageContent::Parts(parts) => {
                    for part in parts {
                        match part {
                            MessagePart::Image { .. } => images += 1.,
                            MessagePart::ToolResult {
                                content: ToolContent::Blocks(blocks),
                                ..
                            } => {
                                for block in blocks {
                                    match block {
                                        ContentBlock::Image { .. } => images += 1.,
                                        block => bytes += size(block),
                                    }
                                }
                            }
                            part => bytes += size(part),
                        }
                    }
                }
            }
        }
        bytes / BYTES_PER_TOKEN + images * IMAGE_TOKENS
    }
}

/// Serialized JSON size; request parts serialize by construction.
fn size(value: &impl serde::Serialize) -> f64 {
    serde_json::to_vec(value).map_or(0, |bytes| bytes.len()) as f64
}
