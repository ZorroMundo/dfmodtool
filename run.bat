taskkill /f /im "DF CONNECTED v2.7.9c.exe"
start "" "%DF_EXECUTABLE%"
cargo run --target=i686-pc-windows-msvc %*