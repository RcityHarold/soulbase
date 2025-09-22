#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthInfo {
    pub ok: bool,
    pub message: String,
}

impl HealthInfo {
    pub fn healthy() -> Self {
        Self {
            ok: true,
            message: "ok".into(),
        }
    }

    pub fn unhealthy(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: msg.into(),
        }
    }
}
