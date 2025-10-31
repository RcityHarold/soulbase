use anyhow::{anyhow, Result};

#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub bind_addr: String,
}

impl GatewayConfig {
    pub fn from_env() -> Result<Self> {
        let bind_addr =
            std::env::var("SB_GATEWAY_ADDR").unwrap_or_else(|_| "0.0.0.0:8800".to_string());
        if bind_addr.trim().is_empty() {
            return Err(anyhow!("SB_GATEWAY_ADDR 不能为空"));
        }

        Ok(Self { bind_addr })
    }
}
