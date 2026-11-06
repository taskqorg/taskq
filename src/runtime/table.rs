//! The runtime data table (docs/runtime-qualification.md#runtime-adapters): the only place that holds runtime names
//! and native names, as values. `scripts/check.py` refuses them in any other non-test Rust and refuses a branch on
//! one here too.

use std::path::PathBuf;

/// The only runtime list (§ 9 runtime adapters).
/// A runtime taskq can start. A name from the board stays text until it reaches this type: an unknown one has no
/// variant and gets the adapter that answers no.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeId { Claude, Codex, CodexApp, Antigravity }

impl RuntimeId {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::CodexApp, Self::Antigravity];
    pub const fn name(self) -> &'static str {
        match self { Self::Claude => "claude", Self::Codex => "codex", Self::CodexApp => "codex-app", Self::Antigravity => "antigravity" }
    }
    pub fn parse(name: &str) -> Option<Self> { Self::ALL.into_iter().find(|rt| rt.name() == name) }
}

impl std::fmt::Display for RuntimeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.name()) }
}

impl PartialEq<str> for RuntimeId { fn eq(&self, other: &str) -> bool { self.name() == other } }
impl PartialEq<&str> for RuntimeId { fn eq(&self, other: &&str) -> bool { self.name() == *other } }
impl PartialEq<String> for RuntimeId { fn eq(&self, other: &String) -> bool { self.name() == other } }

pub const RUNTIMES: [&str; 4] = [RuntimeId::Claude.name(), RuntimeId::Codex.name(), RuntimeId::CodexApp.name(), RuntimeId::Antigravity.name()];
/// §4 fallback order, excluding the explicitly selected app runtime.
pub const CLI_RUNTIMES: [&str; 3] = [RuntimeId::Claude.name(), RuntimeId::Codex.name(), RuntimeId::Antigravity.name()];

/// What a runtime can do; [`Adapter::has`] answers from the table, and no name of a capability reaches a wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    /// Runs as a service child.
    Cli,
    /// Needs an operator grant; a request without one never admits it.
    Grant,
    /// Claude Remote Control links, permission-mode args, attempt id as session.
    RemoteControl,
    /// Codex app turn, macOS and Windows opt-in (§ 9).
    AppTurn,
    /// Host Codex sandbox args and native executor.
    SandboxArgs,
    /// Codex threads, rollouts and app-owned input.
    ThreadLink,
    /// Claude's pre-launch session id.
    CallerSessionId,
    /// Claude's background job.
    Background,
    /// JSON input whose stream names the session.
    StreamInput,
    /// A user-installed binary.
    UserBin,
    /// § 3 runtime accounts.
    Accounts,
    /// JSON login status.
    JsonStatus,
    /// Codex `auth.json`.
    AuthFile,
    /// Codex CLI resume releases the app's writer lock.
    PassiveWriter,
    /// Rate limits from the rollout.
    RolloutQuota,
    /// Codex model list.
    ModelCatalog,
    /// An account's usage through the app-server call.
    UsageRpc,
    /// An account's usage through the provider's OAuth usage call.
    UsageOauth,
    /// An account's usage through the Code Assist quota calls with the `agy` login.
    UsageCodeAssist,
    /// Launches need the project's trust entry.
    ProjectTrust,
}

/// One adapter per runtime (docs/runtime-qualification.md#runtime-adapters): data only. Code outside this table asks
/// it for a capability or a value, never compares runtime names (#2271). Capability names follow the AgentQ runtime
/// trait's descriptors (`agentq_runtime::agent::adapter`) where they overlap.
pub struct Adapter {
    pub name: &'static str,
    /// CLI binary and login account; `codex-app` shares Codex's.
    pub bin: &'static str,
    pub client: &'static str,
    pub home: &'static str,
    pub caps: &'static [Cap],
    /// The AgentQ client that launches this runtime.
    /// AgentQ descriptor name (`agentq_runtime::agent::runtime`).
    pub agent: &'static str,
    /// The native home and its override variable (§ 3 Runtime accounts).
    pub native_home: NativeHome,
    pub home_var: &'static str,
    /// Login status args (empty: no CLI probe), account login args and the re-login step ([runbook](../docs/relogin.md)).
    pub status: &'static [&'static str],
    pub device_login: &'static [&'static str],
    pub relogin: &'static str,
    /// Runtime whose access a setup launch of this client uses; empty: no setup.
    pub setup: &'static str,
    pub login: &'static str,
    pub sessions: &'static str,
    /// Oldest qualified CLI version and the owner's update step ([host tools](../docs/design/host-service.md#host-tools)).
    pub min: &'static str,
    pub update: &'static str,
    /// AgentQ `history` format of the native transcript (#2176).
    pub history: &'static str,
}

impl Adapter {
    pub fn has(&self, cap: Cap) -> bool { self.caps.contains(&cap) }
}

/// Which native home a runtime resolves through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeHome { Claude, Codex }

impl NativeHome {
    pub fn path(self) -> Option<PathBuf> {
        match self { Self::Claude => super::claude_home(), Self::Codex => super::codex_home() }
    }
}

const CODEX_UPDATE: &str = "update Codex CLI (https://github.com/openai/codex#installation)";
const CODEX_RELOGIN: &str = "run `codex login --device-auth` with the PM (docs/relogin.md)";
pub(super) const ADAPTERS: [Adapter; 4] = [
    Adapter { name: RuntimeId::Claude.name(), bin: "claude", client: "Claude Code", home: ".claude",
        caps: &[Cap::Cli, Cap::RemoteControl, Cap::CallerSessionId, Cap::Background, Cap::Accounts, Cap::JsonStatus, Cap::ProjectTrust, Cap::UsageOauth],
        agent: "claude", native_home: NativeHome::Claude, home_var: "CLAUDE_CONFIG_DIR", status: &["auth", "status"], device_login: &["auth", "login"],
        relogin: "run `claude auth login` with the PM (docs/relogin.md)", setup: RuntimeId::Claude.name(), login: "claude auth login", sessions: "`claude agents`",
        min: "2.1.296", update: "run `claude update`", history: "claude-native" },
    Adapter { name: RuntimeId::Codex.name(), bin: "codex", client: "Codex", home: ".codex",
        caps: &[Cap::Cli, Cap::SandboxArgs, Cap::ThreadLink, Cap::Accounts, Cap::AuthFile, Cap::PassiveWriter, Cap::RolloutQuota, Cap::ModelCatalog, Cap::UsageRpc],
        agent: "codex", native_home: NativeHome::Codex, home_var: CODEX_DIR_VAR, status: &["login", "status"], device_login: &["login", "--device-auth"],
        relogin: CODEX_RELOGIN, setup: RuntimeId::CodexApp.name(), login: "codex login", sessions: "the Codex app threads",
        min: "0.159.3", update: CODEX_UPDATE, history: "codex-rollout" },
    Adapter { name: RuntimeId::CodexApp.name(), bin: "codex", client: "Codex", home: ".codex",
        caps: &[Cap::AppTurn, Cap::SandboxArgs, Cap::ThreadLink, Cap::AuthFile, Cap::ModelCatalog],
        agent: "codex", native_home: NativeHome::Codex, home_var: CODEX_DIR_VAR, status: &["login", "status"], device_login: &["login", "--device-auth"],
        relogin: CODEX_RELOGIN, setup: "", login: "codex login", sessions: "the Codex app threads",
        min: "0.159.3", update: CODEX_UPDATE, history: "codex-rollout" },
    Adapter { name: RuntimeId::Antigravity.name(), bin: "agy", client: "Antigravity", home: ".gemini/antigravity",
        caps: &[Cap::Cli, Cap::Grant, Cap::StreamInput, Cap::UserBin, Cap::Accounts, Cap::UsageCodeAssist],
        agent: "antigravity", native_home: NativeHome::Codex, home_var: "", status: &[], device_login: &[],
        relogin: "sign in through the Antigravity client", setup: RuntimeId::Antigravity.name(), login: "sign in through the Antigravity client", sessions: "Antigravity conversations",
        min: "", update: "install or update the Antigravity agy CLI", history: "agy-stream" },
];
// ponytail: an unknown name keeps Codex-shaped login defaults (`login status`, Codex's home variable)
pub(super) const OTHER: Adapter = Adapter { name: "", bin: "", client: "", home: "", caps: &[], agent: "", native_home: NativeHome::Codex, home_var: CODEX_DIR_VAR,
    status: &["login", "status"], device_login: &["login", "--device-auth"], relogin: CODEX_RELOGIN, setup: "", login: "", sessions: "", min: "", update: "", history: "claude-native" };

/// The CLI runtimes a host can run as service children, one per binary: the runtime rows of host tools.
pub fn clis() -> impl Iterator<Item = &'static Adapter> {
    ADAPTERS.iter().filter(|a| a.has(Cap::Cli))
}

/// The adapter of `rt`; an unknown name gets one that answers no to every question.
pub fn of(rt: &str) -> &'static Adapter {
    ADAPTERS.iter().find(|a| a.name == rt).unwrap_or(&OTHER)
}

/// The runtimes code names directly for their own native paths; every other question goes to [`of`].
pub const CLAUDE: &str = RuntimeId::Claude.name();
pub const CODEX: &str = RuntimeId::Codex.name();
/// Claude's config file in the user's home: project trust lives there.
pub const CLAUDE_CONFIG: &str = ".claude.json";
/// Claude's process image on Windows.
pub const CLAUDE_EXE: &str = "claude.exe";
/// The variable that overrides Codex's home.
pub const CODEX_DIR_VAR: &str = "CODEX_HOME";
/// Codex app thread links.
pub const THREADS: &str = "codex://threads/";
/// Owner sessions run Claude Code.
pub const OWNER: &str = RuntimeId::Claude.name();
/// The host config key that holds Codex CLI arguments.
pub const CODEX_ARGS: &str = "codex";

/// Runtimes a setup request may name, Codex first as the Connect list shows them.
pub fn setup_runtimes() -> impl Iterator<Item = &'static str> {
    ADAPTERS.iter().rev().filter(|a| !a.setup.is_empty()).map(|a| a.name)
}

/// The runtimes taskq can start here (§ 8): `codex-app` is macOS and Windows only and shares Codex's binary and login.
pub fn agent_runtimes() -> &'static [&'static str] {
    if cfg!(any(target_os = "macos", windows)) { &RUNTIMES } else { &CLI_RUNTIMES }
}
