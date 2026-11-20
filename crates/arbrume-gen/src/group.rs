//! Stands: several plants grown together in one environment, then meshed one by one.

use std::sync::Arc;

use arbrume_core::species::Species;
use arbrume_grow::{Standing, Surroundings, grow_together};
use glam::Vec3;

use crate::batch::{Budget, Generator};
use crate::params::{GenError, GenParams};
use crate::plant::Plant;

/// One plant of a stand for [`Generator::generate_group`]: its species, its parameters
/// and where it stands, in meters (X, Y up, Z).
pub type GroupEntry<'a> = (&'a Species, GenParams, [f32; 3]);

impl Generator {
    /// Grows the plants of `entries` together, so that they shade one another
    /// (`arbrume_grow::grow_together`), and meshes each. Returns one plant per entry, in
    /// the order of `entries`, each in its own frame with its base at the origin: place
    /// the plant of entry `i` at `entries[i].2`.
    ///
    /// The surroundings of a stand are in the frame of the stand, not of a plant: every
    /// entry gives the same `surroundings` (or none). The result does not depend on the
    /// order of the entries, nor on the number of threads; a stand of one plant at the
    /// origin gives the plant of [`Generator::generate`].
    ///
    /// A species with a `[roots]` layer grows its roots after the stand, in the soil of
    /// its parameters, without the obstacles of the stand.
    ///
    /// # Errors
    ///
    /// The errors of [`Generator::generate`] for any entry, and
    /// [`GenError::InvalidParam`] when the entries give different surroundings or a
    /// position that is not finite.
    pub fn generate_group(&mut self, entries: &[GroupEntry]) -> Result<Vec<Plant>, GenError> {
        let surroundings = group_surroundings(entries)?;
        let open = Surroundings::none();
        let standing = entries
            .iter()
            .map(|(species, params, position)| standing(species, params, *position))
            .collect::<Result<Vec<_>, _>>()?;
        let graphs = grow_together(&standing, surroundings.as_deref().unwrap_or(&open));
        let mut plants = Vec::with_capacity(entries.len());
        for ((species, params, _), graph) in entries.iter().zip(graphs) {
            let mut plant = Plant::default();
            let graph = crate::rooted::add(species, params, &open, graph);
            self.mesh_grown(species, params, &Budget::NONE, graph, &mut plant)?;
            plants.push(plant);
        }
        Ok(plants)
    }
}

/// The checked plant of a stand.
fn standing<'a>(
    species: &'a Species,
    params: &GenParams,
    position: [f32; 3],
) -> Result<Standing<'a>, GenError> {
    species.validate()?;
    params.validate()?;
    let position = Vec3::from_array(position);
    if !position.is_finite() {
        return Err(GenError::InvalidParam {
            field: "position",
            reason: format!("{position} is not finite"),
        });
    }
    Ok(Standing {
        species,
        params: params.grow_params(),
        position,
    })
}

/// The surroundings shared by every entry.
fn group_surroundings(entries: &[GroupEntry]) -> Result<Option<Arc<Surroundings>>, GenError> {
    let Some((_, first, _)) = entries.first() else {
        return Ok(None);
    };
    let shared = &first.surroundings;
    if entries
        .iter()
        .any(|(_, params, _)| &params.surroundings != shared)
    {
        return Err(GenError::InvalidParam {
            field: "surroundings",
            reason: "the plants of a stand share one surroundings".to_owned(),
        });
    }
    Ok(shared.clone())
}
