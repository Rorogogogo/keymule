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

/// What kind of secret a preset holds, which decides how it can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An API token keymule attaches to HTTP requests (and can hand to a CLI).
    Http,
    /// A password: only handed to a program, in an env var.
    Password,
    /// A private key (PEM): handed to a program as a temporary file, or in
    /// an env var.
    PrivateKey,
    /// An AWS access key ID and secret, stored as two lines: handed to a
    /// program as AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.
    AwsKeys,
    /// An SSH login: a password or private key for one server and account,
    /// only used by `keymule ssh`.
    Ssh,
    /// A website's username and password, only filled into that site's
    /// sign-in page in the keymule browser (`sign_in`).
    Login,
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
    /// More hosts the secret may go to when the URL is `default_url`
    /// (GitHub's git and upload hosts next to its API).
    pub extra_hosts: &'static [&'static str],
    /// The env var the service's own CLI reads its token from, for
    /// `keymule exec <name> --env VAR -- <cli>`.
    pub cli_env: Option<(&'static str, &'static str)>,
    pub kind: Kind,
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
        extra_hosts: &[],
        cli_env: None,
        kind: Kind::Http,
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
        extra_hosts: &["github.com", "uploads.github.com"],
        cli_env: Some(("GH_TOKEN", "gh")),
        kind: Kind::Http,
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
        extra_hosts: &[],
        cli_env: Some(("GITLAB_TOKEN", "glab")),
        kind: Kind::Http,
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
        extra_hosts: &[],
        cli_env: None,
        kind: Kind::Http,
    },
    Preset {
        id: "stripe",
        title: "Stripe",
        default_url: Some("https://api.stripe.com"),
        url_example: "https://api.stripe.com",
        auth: AuthKind::Bearer,
        verify_path: Some("/v1/balance"),
        accept: "application/json",
        keywords: &["stripe"],
        hint: "Stripe REST API (form-encoded bodies, not JSON). \
               Check the key: GET /v1/balance. \
               Products and prices: GET /v1/products?limit=10, GET /v1/prices?product={id}. \
               Subscriptions: GET /v1/subscriptions?limit=10&status=all, GET /v1/subscriptions/{id}. \
               Customers: GET /v1/customers?email={email}. \
               Webhooks: GET /v1/webhook_endpoints. \
               Recent failures: GET /v1/events?type=invoice.payment_failed&limit=10.",
        extra_hosts: &[],
        cli_env: Some(("STRIPE_API_KEY", "stripe")),
        kind: Kind::Http,
    },
    Preset {
        id: "openai",
        title: "OpenAI",
        default_url: Some("https://api.openai.com/v1"),
        url_example: "https://api.openai.com/v1",
        auth: AuthKind::Bearer,
        verify_path: Some("/models"),
        accept: "application/json",
        keywords: &["openai", "chatgpt"],
        hint: "OpenAI REST API. Models: GET /models. \
               Responses: POST /responses with {\"model\": \"...\", \"input\": \"...\"}.",
        extra_hosts: &[],
        cli_env: Some(("OPENAI_API_KEY", "openai")),
        kind: Kind::Http,
    },
    Preset {
        id: "password",
        title: "Password",
        default_url: None,
        url_example: "",
        auth: AuthKind::Bearer,
        verify_path: None,
        accept: "",
        keywords: &[], // found by their own shape, not by words nearby
        hint: "A password. It isn't for HTTP requests: hand it to a program with \
               `keymule exec <name> --env VAR -- <command>` (e.g. --env PGPASSWORD -- psql …).",
        extra_hosts: &[],
        cli_env: None,
        kind: Kind::Password,
    },
    Preset {
        id: "private-key",
        title: "Private key",
        default_url: None,
        url_example: "",
        auth: AuthKind::Bearer,
        verify_path: None,
        accept: "",
        keywords: &[], // found by their own shape, not by words nearby
        hint: "A private key (PEM). Hand it to a program as a file with \
               `keymule exec <name> --file KEY -- ssh -i {KEY} user@host`: keymule writes it to a \
               temporary file only you can read, puts that file's path where {KEY} is (and in \
               the KEY env var), and deletes it when the program ends.",
        extra_hosts: &[],
        cli_env: None,
        kind: Kind::PrivateKey,
    },
    Preset {
        id: "ssh",
        title: "SSH login",
        default_url: None,
        url_example: "",
        auth: AuthKind::Bearer,
        verify_path: None,
        accept: "",
        keywords: &[], // added with add_ssh_credential, not guessed from a paste
        hint: "An SSH login for one server. Run commands there with \
               `keymule ssh <name> -- <command>` (e.g. -- uptime): keymule runs ssh and answers \
               its password or key prompt itself, only for that server and account. The user \
               approves it once per folder. Not for http_request.",
        extra_hosts: &[],
        cli_env: None,
        kind: Kind::Ssh,
    },
    Preset {
        id: "login",
        title: "Website login",
        default_url: None,
        url_example: "https://github.com/login",
        auth: AuthKind::Basic,
        verify_path: None,
        accept: "",
        keywords: &[], // added with add_website_login, not guessed from a paste
        hint: "A username and password for one website. Sign in with \
               sign_in(credential=\"<name>\") once the site's sign-in page is open in the keymule \
               browser: keymule asks the user, then types them into that page itself, only on that \
               site. Not for http_request or keymule exec.",
        extra_hosts: &[],
        cli_env: None,
        kind: Kind::Login,
    },
    Preset {
        id: "aws",
        title: "AWS access keys",
        default_url: None,
        url_example: "",
        auth: AuthKind::Bearer,
        verify_path: None,
        accept: "",
        keywords: &[], // found by their own shape, not by words nearby
        hint: "An AWS access key ID and secret. `keymule exec <name> -- aws s3 ls` (or any AWS \
               tool) gets AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.",
        extra_hosts: &[],
        cli_env: None,
        kind: Kind::AwsKeys,
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
        extra_hosts: &[],
        cli_env: None,
        kind: Kind::Http,
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

/// The hosts a credential for `preset` at `url` may be sent to: the URL's
/// host, plus the preset's extra hosts when it's the preset's own URL.
pub fn hosts_for(preset: &Preset, url: &str) -> anyhow::Result<Vec<String>> {
    if url.is_empty() {
        return Ok(vec![]);
    }
    let host = crate::store::host_of(url)?;
    let mut hosts = vec![host.clone()];
    let default = preset.default_url.map(crate::store::host_of).transpose()?;
    if default.as_deref() == Some(host.as_str()) {
        hosts.extend(preset.extra_hosts.iter().map(|h| h.to_string()));
    }
    Ok(hosts)
}

pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

pub fn get_or_generic(id: &str) -> &'static Preset {
    find(id).unwrap_or_else(|| find("generic").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_covers_git_and_uploads_but_enterprise_keeps_its_one_host() {
        let gh = find("github").unwrap();
        assert_eq!(
            hosts_for(gh, "https://api.github.com").unwrap(),
            ["api.github.com", "github.com", "uploads.github.com"]
        );
        assert_eq!(
            hosts_for(gh, "https://github.corp.com/api/v3").unwrap(),
            ["github.corp.com"]
        );
        assert_eq!(
            hosts_for(find("gitlab").unwrap(), "https://gitlab.com/api/v4").unwrap(),
            ["gitlab.com"]
        );
        assert!(hosts_for(gh, "").unwrap().is_empty());
    }
}
