//! The self-organizing presets on open ground, which the shade of occluders must not
//! change when there is no occluder.

mod self_organizing_snapshot;

#[test]
fn self_organizing_presets_match_the_occluder_golden_file() {
    self_organizing_snapshot::check("occluder");
}
