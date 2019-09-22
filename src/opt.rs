use std::path::PathBuf;
pub use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "weo", about = "A simple WAN emulator in Rust.")]
pub struct WeoOpts {
    #[structopt(short = "p", long, value_name = "PORT", default_value = "88")]
    pub port: u16,

    #[structopt(short = "w", long, value_name = "WEB_ROOT", parse(from_os_str))]
    pub root: PathBuf,
}
