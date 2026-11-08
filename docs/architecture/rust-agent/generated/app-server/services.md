# App Server 进程组合

由 `serve` 以产品配置（`--product-home`）真实装配生成，只含进程层，不含 Agent 实例。重新生成：`cargo run -p xtask -- graph`。

```mermaid
%% Generated from live rutis diagnostics; do not edit.
flowchart LR
  subgraph process["process"]
    p1["root"]
    p2["rutis-loader"]
    p68["Attachments"]
    p67["goal"]
    p66["Questions"]
    p65["PromptSuggestion"]
    p64["PrepareModelHost"]
    p63["ContextCompaction"]
    p62["CommandRules"]
    p61["PromptSources"]
    p60["ShellInput"]
    p59["Recall"]
    p58["Wait"]
    p57["WorkspaceTools"]
    p56["BranchTool"]
    p55["ToolExecution"]
    p54["Permission"]
    p53["ModelProvider"]
    p52["Loop"]
    p51["Prompt"]
    p50["ContextPreparation"]
    p49["ToolRegistry"]
    p48["InputQueue"]
    p47["SessionMethods"]
    p46["TimelineTree"]
    p45["SessionBroadcast"]
    p44["session-title"]
    p43["PartRegistry"]
    p42["Transport"]
    p41["Rpc"]
    p40["Connection"]
    p39["UiHost"]
    p38["Collaboration"]
    p37["Agents"]
    p36["CollaborationDirectory"]
    p35["AgentDirectory"]
    p34["PluginManager"]
    p33["CollaborationModelPolicy"]
    p32["HooksSettings"]
    p31["PermissionRules"]
    p30["SkillsCatalog"]
    p29["ExecutionBackend"]
    p28["GitWorkspace"]
    p27["Terminals"]
    p26["ExecShellSettings"]
    p25["HostPlatform"]
    p24["PromptSuggestionSettings"]
    p23["ExecutionSettings"]
    p22["lsp"]
    p21["Mcp"]
    p20["McpPublication"]
    p19["ModelLog"]
    p18["web"]
    p17["SettingsMethods"]
    p16["Settings"]
    p15["Modality"]
    p14["Provider"]
    p13["GrokOAuth"]
    p12["OpenAICodexOAuth"]
    p11["ManagedProviderMethods"]
    p10["PluginFiles"]
    p9["ManagedProviders"]
    p8["ManagementMethods"]
    p7["SessionStore"]
    p6["SqliteStorage"]
    p5["Aimux"]
    p4["FileStorage"]
    p3["ServerControl"]
  end
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p68
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p67
  p16 -->|"dim_app_server::settings::SettingsService"| p67
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p66
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p65
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p64
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p63
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p62
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p61
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p60
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p59
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p58
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p57
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p56
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p55
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p54
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p53
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p52
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p51
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p50
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p49
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p48
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p47
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p46
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p45
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p44
  p16 -->|"dim_app_server::settings::SettingsService"| p44
  p40 -->|"dim_app_server::connection::ConnectionHub"| p42
  p3 -->|"dim_app_server::control::ServerControl"| p42
  p6 -->|"dim_app_server::storage::sqlite::SqliteStorage"| p42
  p37 -->|"dim_app_server::agents::AgentIndex"| p41
  p38 -->|"dim_app_server::collaboration::Collaboration"| p41
  p40 -->|"dim_app_server::connection::ConnectionHub"| p41
  p3 -->|"dim_app_server::control::ServerControl"| p41
  p8 -->|"dim_app_server::methods::ManagementMethods"| p41
  p2 -->|"rutis_loader::loader::Loader"| p41
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p40
  p3 -->|"dim_app_server::control::ServerControl"| p40
  p39 -->|"dim_app_server::ui_host::UiHost"| p40
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p38
  p37 -->|"dim_app_server::agents::AgentIndex"| p38
  p36 -->|"dim_app_server::collaboration::CollaborationDirectory"| p38
  p16 -->|"dim_app_server::settings::SettingsService"| p38
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p37
  p35 -->|"dim_app_server::agents::AgentDirectory"| p37
  p16 -->|"dim_app_server::settings::SettingsService"| p37
  p6 -->|"dim_app_server::storage::sqlite::SqliteStorage"| p37
  p7 -->|"dim_app_server::storage::timeline::SessionStore"| p37
  p2 -->|"rutis_loader::loader::Loader"| p37
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p34
  p8 -->|"dim_app_server::methods::ManagementMethods"| p34
  p2 -->|"rutis_loader::loader::Loader"| p34
  p8 -->|"dim_app_server::methods::ManagementMethods"| p33
  p16 -->|"dim_app_server::settings::SettingsService"| p33
  p4 -->|"dim_agent_capabilities::file_storage::FileStorage"| p32
  p8 -->|"dim_app_server::methods::ManagementMethods"| p32
  p16 -->|"dim_app_server::settings::SettingsService"| p32
  p8 -->|"dim_app_server::methods::ManagementMethods"| p31
  p16 -->|"dim_app_server::settings::SettingsService"| p31
  p25 -->|"dim_agent_capabilities::host_platform::HostPlatform"| p30
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p30
  p8 -->|"dim_app_server::methods::ManagementMethods"| p30
  p16 -->|"dim_app_server::settings::SettingsService"| p30
  p25 -->|"dim_agent_capabilities::host_platform::HostPlatform"| p29
  p8 -->|"dim_app_server::methods::ManagementMethods"| p29
  p8 -->|"dim_app_server::methods::ManagementMethods"| p28
  p25 -->|"dim_agent_capabilities::host_platform::HostPlatform"| p27
  p8 -->|"dim_app_server::methods::ManagementMethods"| p27
  p25 -->|"dim_agent_capabilities::host_platform::HostPlatform"| p26
  p8 -->|"dim_app_server::methods::ManagementMethods"| p26
  p23 -->|"dim_agent_capabilities::execution_settings::ExecutionSettingsHost"| p25
  p4 -->|"dim_agent_capabilities::file_storage::FileStorage"| p25
  p16 -->|"dim_app_server::settings::SettingsService"| p24
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p23
  p16 -->|"dim_app_server::settings::SettingsService"| p23
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p22
  p16 -->|"dim_app_server::settings::SettingsService"| p22
  p4 -->|"dim_agent_capabilities::file_storage::FileStorage"| p21
  p1 -->|"dim_agent_capabilities::host::CredentialVariablesHost"| p21
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p21
  p20 -->|"dim_app_server::mcp::publication::Publication"| p21
  p8 -->|"dim_app_server::methods::ManagementMethods"| p21
  p16 -->|"dim_app_server::settings::SettingsService"| p21
  p1 -->|"dim_agent_capabilities::host::CredentialVariablesHost"| p20
  p16 -->|"dim_app_server::settings::SettingsService"| p20
  p4 -->|"dim_agent_capabilities::file_storage::FileStorage"| p19
  p16 -->|"dim_app_server::settings::SettingsService"| p19
  p43 -->|"dim_agent_runtime::parts::PartRegistry"| p18
  p16 -->|"dim_app_server::settings::SettingsService"| p18
  p8 -->|"dim_app_server::methods::ManagementMethods"| p17
  p16 -->|"dim_app_server::settings::SettingsService"| p17
  p4 -->|"dim_agent_capabilities::file_storage::FileStorage"| p16
  p6 -->|"dim_app_server::storage::sqlite::SqliteStorage"| p16
  p14 -->|"dim_agent_capabilities::host::ModelSelectionHost"| p15
  p14 -->|"dim_agent_capabilities::host::SelectableModelsHost"| p15
  p8 -->|"dim_app_server::methods::ManagementMethods"| p15
  p16 -->|"dim_app_server::settings::SettingsService"| p15
  p5 -->|"dim_agent_capabilities::host::ModelHost"| p14
  p37 -->|"dim_app_server::agents::AgentIndex"| p14
  p12 -->|"dim_app_server::codex_oauth::OpenAICodexOAuthService"| p14
  p13 -->|"dim_app_server::grok_oauth::GrokOAuthService"| p14
  p9 -->|"dim_app_server::managed_provider::ManagedProviders"| p14
  p8 -->|"dim_app_server::methods::ManagementMethods"| p14
  p6 -->|"dim_app_server::storage::sqlite::SqliteStorage"| p14
  p4 -->|"dim_agent_capabilities::file_storage::FileStorage"| p13
  p8 -->|"dim_app_server::methods::ManagementMethods"| p13
  p4 -->|"dim_agent_capabilities::file_storage::FileStorage"| p12
  p8 -->|"dim_app_server::methods::ManagementMethods"| p12
  p9 -->|"dim_app_server::managed_provider::ManagedProviders"| p11
  p8 -->|"dim_app_server::methods::ManagementMethods"| p11
  p4 -->|"dim_agent_capabilities::file_storage::FileStorage"| p10
  p6 -->|"dim_app_server::storage::sqlite::SqliteStorage"| p7
```
