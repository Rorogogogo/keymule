//! Finding secrets: in a message about to be sent, and ones that already
//! leaked (agent transcripts, configs, .env files, shell history).

pub mod detect;
pub mod doctor;
