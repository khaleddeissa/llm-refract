#[tokio::main]
async fn main() -> anyhow::Result<()> {
    refract_server::serve().await
}
