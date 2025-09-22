use crate::model::{Cost, CostBreakdown, Usage};

/// Basic heuristic usage estimator for the local provider.
pub fn estimate_usage(inputs: &[&str], output: &str) -> Usage {
    let input_tokens = inputs
        .iter()
        .map(|s| ((s.chars().count() as u32) + 3) / 4)
        .sum();
    let output_tokens = ((output.chars().count() as u32) + 3) / 4;
    Usage {
        input_tokens,
        output_tokens,
        cached_tokens: None,
        image_units: None,
        audio_seconds: None,
        requests: 1,
    }
}

pub fn zero_cost() -> Option<Cost> {
    Some(Cost {
        usd: 0.0,
        currency: "USD".to_string(),
        breakdown: CostBreakdown {
            input: 0.0,
            output: 0.0,
            image: 0.0,
            audio: 0.0,
        },
    })
}
