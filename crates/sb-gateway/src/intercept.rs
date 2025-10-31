use axum::body::Body;
use axum::http::{Request, Response};
use sb_interceptors::adapters::http::{AxumRequest, AxumResponse};
use sb_interceptors::errors::InterceptError;
use sb_interceptors::prelude::{
    ContextInitStage, InterceptContext, InterceptorChain, ResponseStampStage, Stage,
};
use sb_interceptors::stages::ResponseStage;
use std::future::Future;

pub struct InterceptorFacade {
    chain: InterceptorChain,
}

impl InterceptorFacade {
    pub fn new() -> Self {
        let request_stages: Vec<Box<dyn Stage>> =
            vec![Box::new(ContextInitStage::default()) as Box<dyn Stage>];
        let response_stages: Vec<Box<dyn ResponseStage>> =
            vec![Box::new(ResponseStampStage) as Box<dyn ResponseStage>];
        let chain = InterceptorChain::new(request_stages, response_stages);
        Self { chain }
    }

    pub async fn execute<F, Fut>(
        &self,
        request: Request<Body>,
        handler: F,
    ) -> Result<Response<Body>, InterceptError>
    where
        F: FnMut(&mut InterceptContext, &mut dyn sb_interceptors::context::ProtoRequest) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = Result<serde_json::Value, InterceptError>> + Send + 'static,
    {
        let mut req = AxumRequest::new(request);
        let mut rsp = AxumResponse::new();
        let cx = InterceptContext::new();

        self.chain
            .run_with_handler(cx, &mut req, &mut rsp, handler)
            .await?;

        Ok(rsp.into_response())
    }
}
