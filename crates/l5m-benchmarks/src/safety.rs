use crate::runfile::RunRow;

pub fn render_safety_markdown(rows: &[RunRow]) -> String {
    let summary = summarize_safety(rows);
    let mut out = String::from("# L5M Safety Scorecard\n\n");
    out.push_str("| Queries | Tenant | Context | Policy | Trust | Temporal | Poison | Total Violating Rows |\n");
    out.push_str("| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    out.push_str(&format!(
        "| {} | {} | {} | {} | {} | {} | {} | {} |\n\n",
        rows.len(),
        summary.tenant,
        summary.context,
        summary.policy,
        summary.trust,
        summary.temporal,
        summary.poison,
        summary.total
    ));
    out.push_str("Release requirement: total violating rows must be `0`.\n");
    out
}

#[derive(Default)]
struct SafetySummary {
    tenant: usize,
    context: usize,
    policy: usize,
    trust: usize,
    temporal: usize,
    poison: usize,
    total: usize,
}

fn summarize_safety(rows: &[RunRow]) -> SafetySummary {
    let mut summary = SafetySummary::default();
    for row in rows {
        summary.tenant += usize::from(row.gate_violations.tenant_violation);
        summary.context += usize::from(row.gate_violations.context_violation);
        summary.policy += usize::from(row.gate_violations.policy_violation);
        summary.trust += usize::from(row.gate_violations.trust_violation);
        summary.temporal += usize::from(row.gate_violations.temporal_violation);
        summary.poison += usize::from(row.gate_violations.poison_violation);
        summary.total += usize::from(
            row.gate_violations.tenant_violation
                || row.gate_violations.context_violation
                || row.gate_violations.policy_violation
                || row.gate_violations.trust_violation
                || row.gate_violations.temporal_violation
                || row.gate_violations.poison_violation,
        );
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runfile::RunRow;

    #[test]
    fn safety_scorecard_reports_zero_violations_for_safe_rows() {
        let row = RunRow::minimal("LongMemEval", "q1", "Where?", "hybrid-parent", 10);

        let markdown = render_safety_markdown(&[row]);

        assert!(markdown.contains("Total Violating Rows"));
        assert!(markdown.contains("| 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |"));
    }
}
