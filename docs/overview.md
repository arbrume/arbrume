# Overview

Arbrume grows trees on a plant graph: an arena of metamers (internode, node, leaf, bud).
Each growth cycle runs three stages:

1. **Environment**: none, a shadow grid, or attraction markers.
2. **Bud fate**: which buds grow, rest or die.
3. **Orientation**: where new shoots point.

Species parameters are curves over relative position along a branch and branch order.
A species grows in the parametric mode, where every axis follows those curves, or in
the self-organizing mode, where buds compete for light and space and the plant responds
to an envelope, obstacles and neighbours. L-systems appear only as test fixtures, not as
the runtime.

Planned parts:

| Part | Role |
|---|---|
| Core | Plant graph, random numbers, spatial grid, species schema |
| Growth | The cycle above, environments, bud fate models, radii |
| Mesh | Branch sweep, junctions, twigs, leaves, UVs, LOD |
| Export | glTF (`.glb`) writer and dataset annotations |
| CLI | Batch generation |
| Python and Blender | Bindings and a Blender extension |

Output depends only on species, parameters, seed and version, never on thread count.
