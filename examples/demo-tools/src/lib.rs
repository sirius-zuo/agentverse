pub mod check_refund_eligibility;
pub mod check_service_status;
pub mod count_mentions;
pub mod find_dates;
pub mod get_account_details;
pub mod lookup_invoice;
pub mod market_sizing_calculator;
pub mod milestone_scheduler;
pub mod npv_calculator;
pub mod project_cost_estimator;
pub mod risk_adjusted_schedule;
pub mod runway_projector;
pub mod word_count;
pub mod ledger_post;

pub use check_refund_eligibility::CheckRefundEligibility;
pub use check_service_status::CheckServiceStatus;
pub use count_mentions::CountMentions;
pub use find_dates::FindDates;
pub use get_account_details::GetAccountDetails;
pub use lookup_invoice::LookupInvoice;
pub use market_sizing_calculator::MarketSizingCalculator;
pub use milestone_scheduler::MilestoneScheduler;
pub use npv_calculator::NpvCalculator;
pub use project_cost_estimator::ProjectCostEstimator;
pub use risk_adjusted_schedule::RiskAdjustedSchedule;
pub use runway_projector::RunwayProjector;
pub use word_count::WordCount;
pub use ledger_post::LedgerPost;

pub(crate) fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
