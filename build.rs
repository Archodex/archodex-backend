fn main() -> std::io::Result<()> {
    prost_build::compile_protos(&["src/report_api_key.proto"], &["src/"])?;
    Ok(())
}
