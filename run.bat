taskkill /f /im "DF CONNECTED v2.7.8b.exe"
start "" "%DF_EXECUTABLE%"
cargo run --target=i686-pc-windows-msvc %*