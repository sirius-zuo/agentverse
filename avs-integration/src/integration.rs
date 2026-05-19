use super::connector::{InputConnector, OutputConnector};
use super::error::IntegrationError;
use super::invoker::AgentInvoker;

/// Wires one input, one invoker, and N outputs into a processing loop.
pub struct Integration {
    input: Box<dyn InputConnector>,
    invoker: Box<dyn AgentInvoker>,
    outputs: Vec<Box<dyn OutputConnector>>,
}

impl Integration {
    pub fn new(
        input: Box<dyn InputConnector>,
        invoker: Box<dyn AgentInvoker>,
        outputs: Vec<Box<dyn OutputConnector>>,
    ) -> Self {
        Self { input, invoker, outputs }
    }

    pub async fn run(&self) -> Result<(), IntegrationError> {
        self.input.start().await.map_err(IntegrationError::Input)?;
        loop {
            let event = self.input.receive().await.map_err(IntegrationError::Input)?;
            let response = self
                .invoker
                .invoke(event)
                .await
                .map_err(IntegrationError::Invoker)?;
            let mut errs = vec![];
            for output in &self.outputs {
                if let Err(e) = output.send(response.clone()).await {
                    errs.push(IntegrationError::Output {
                        connector: output.name().to_string(),
                        source: e,
                    });
                }
            }
            if !errs.is_empty() {
                return Err(errs.remove(0));
            }
        }
    }
}
