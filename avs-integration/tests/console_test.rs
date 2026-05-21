use agentverse_integration::console::ConsoleConnector;
use agentverse_integration::{Connector, InputConnector, OutputConnector};

#[test]
fn console_connector_name() {
    assert_eq!(ConsoleConnector::new().name(), "console");
}

// Full receive/send tested via integration — stdin/stdout are hard to unit-test.
// This test verifies the type is constructible and the trait impls exist.
#[test]
fn console_implements_connector_traits() {
    fn assert_input<T: InputConnector>() {}
    fn assert_output<T: OutputConnector>() {}
    assert_input::<ConsoleConnector>();
    assert_output::<ConsoleConnector>();
}
