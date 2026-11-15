//! Using a secret without showing it: HTTP calls, running programs and MCP
//! servers with it, remote MCP headers, git's credential store and GitHub
//! Actions secrets.

pub mod cliexec;
pub mod ghsecret;
pub mod http;
pub mod install;
pub mod remote;
pub mod servers;
