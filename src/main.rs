use hudhook::inject::Process;
use std::env;

fn main() {
    let mut dllp = env::current_exe().unwrap().parent().unwrap().to_path_buf();
    dllp.push("libdfmodtool.dll");
    Process::by_name("DF CONNECTED v2.7.8b.exe").unwrap().inject(dllp).unwrap();
}
