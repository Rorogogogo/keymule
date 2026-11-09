//! Known services: where their API lives, how they authenticate, how to
//! verify a token, and a short API cheat-sheet handed to the agent.

use crate::store::Auth;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    Bearer,
    /// Basic auth; the prompt asks for a username.
    Basic,
    Header(&'static str),
}

#[derive(Debug)]
pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    pub default_url: Option<&'static str>,
    pub url_example: &'static str,
    pub auth: AuthKind,
    /// GET path that succeeds only with a valid token.
    pub verify_path: Option<&'static str>,
    /// Sent as `Accept` on every request.
    pub accept: &'static str,
    /// Words in a prompt that suggest this service (for the paste hook).
    pub keywords: &'static [&'static str],
    pub hint: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "bamboo",
        title: "Atlassian Bamboo",
        default_url: None,
        url_example: "https://bamboo.yourcompany.com",
        auth: AuthKind::Bearer,
        verify_path: Some("/rest/api/latest/currentUser"),
        accept: "application/json",
        keywords: &["bamboo"],
        hint: "Bamboo REST API (Server/Data Center). Plan keys look like PROJ-PLAN, \
               build result keys like PROJ-PLAN-123, job keys like PROJ-PLAN-JOB1. \
               List plans: GET /rest/api/latest/plan?max-results=100. \
               Recent results of a plan: GET /rest/api/latest/result/PROJ-PLAN?max-results=5&expand=results.result. \
               One build with stages and jobs: GET /rest/api/latest/result/PROJ-PLAN-123?expand=stages.stage.results.result. \
               Job log (plain text): GET /download/PROJ-PLAN-JOB1/build_logs/PROJ-PLAN-JOB1-123.log. \
               Queue a build: POST /rest/api/latest/queue/PROJ-PLAN.",
    },
    Preset {
        id: "github",
        title: "GitHub (incl. Actions)",
        default_url: Some("https://api.github.com"),
        url_example: "https://api.github.com  (GitHub Enterprise: https://github.corp.com/api/v3)",
        auth: AuthKind::Bearer,
        verify_path: Some("/user"),
        accept: "application/vnd.github+json",
        keywords: &["github", "gh actions", "github actions", "workflow"],
        hint: "GitHub REST API. Actions: \
               GET /repos/{owner}/{repo}/actions/runs?per_page=5&branch=main, \
               GET /repos/{owner}/{repo}/actions/runs/{run_id}/jobs, \
               GET /repos/{owner}/{repo}/actions/jobs/{job_id}/logs (plain text, follows a redirect), \
               POST /repos/{owner}/{repo}/actions/runs/{run_id}/rerun-failed-jobs. \
               PRs: GET /repos/{owner}/{repo}/pulls?state=open.",
    },
    Preset {
        id: "gitlab",
        title: "GitLab",
        default_url: Some("https://gitlab.com/api/v4"),
        url_example: "https://gitlab.com/api/v4  (self-hosted: https://gitlab.corp.com/api/v4)",
        auth: AuthKind::Header("PRIVATE-TOKEN"),
        verify_path: Some("/user"),
        accept: "application/json",
        keywords: &["gitlab"],
        hint: "GitLab REST API v4. Project ids may be URL-encoded paths (group%2Fproject). \
               Pipelines: GET /projects/{id}/pipelines?per_page=5, \
               GET /projects/{id}/pipelines/{pipeline_id}/jobs, \
               GET /projects/{id}/jobs/{job_id}/trace (plain text log), \
               POST /projects/{id}/pipelines/{pipeline_id}/retry.",
    },
    Preset {
        id: "jenkins",
        title: "Jenkins",
        default_url: None,
        url_example: "https://jenkins.yourcompany.com",
        auth: AuthKind::Basic,
        verify_path: Some("/me/api/json"),
        accept: "application/json",
        keywords: &["jenkins"],
        hint: "Jenkins JSON API (the secret is a user API token). \
               Job status: GET /job/{name}/api/json?tree=lastBuild[number,result,url]. \
               Build: GET /job/{name}/{number}/api/json. \
               Console log: GET /job/{name}/{number}/consoleText. \
               Folders nest as /job/{folder}/job/{name}.",
    },
    Preset {
        id: "generic",
        title: "Any HTTP API",
        default_url: None,
        url_example: "https://api.example.com",
        auth: AuthKind::Bearer,
        verify_path: None,
        accept: "application/json",
        keywords: &[],
        hint: "",
    },
];

/// How a new credential for `preset` authenticates. `header` overrides the
/// preset; a username switches bearer services to basic auth.
pub fn auth_for(
    preset: &Preset,
    header: Option<String>,
    username: Option<String>,
) -> anyhow::Result<Auth> {
    Ok(match (header, preset.auth, username) {
        (Some(h), _, _) => Auth::Header { name: h },
        (None, AuthKind::Header(h), _) => Auth::Header { name: h.into() },
        (None, AuthKind::Basic | AuthKind::Bearer, Some(u)) => Auth::Basic { username: u },
        (None, AuthKind::Basic, None) => anyhow::bail!("{} needs a username", preset.title),
        (None, AuthKind::Bearer, None) => Auth::Bearer,
    })
}

pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

pub fn get_or_generic(id: &str) -> &'static Preset {
    find(id).unwrap_or_else(|| find("generic").unwrap())
}
