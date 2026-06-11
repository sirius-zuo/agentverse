pub mod market_sizing_calculator;
pub mod milestone_scheduler;
pub mod npv_calculator;
pub mod project_cost_estimator;
pub mod risk_adjusted_schedule;
pub mod runway_projector;

pub use market_sizing_calculator::MarketSizingCalculator;
pub use milestone_scheduler::MilestoneScheduler;
pub use npv_calculator::NpvCalculator;
pub use project_cost_estimator::ProjectCostEstimator;
pub use risk_adjusted_schedule::RiskAdjustedSchedule;
pub use runway_projector::RunwayProjector;

pub(crate) fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
