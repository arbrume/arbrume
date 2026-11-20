//! The self-organizing presets on open ground, which the way markers are given to buds
//! must not change.

mod self_organizing_snapshot;

#[test]
fn self_organizing_presets_match_the_marker_golden_file() {
    self_organizing_snapshot::check("marker_grid");
}
