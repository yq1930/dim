# dim-goal

Goal is a rutis plugin installed by the `dimcode` product profile through
`install(&mut AgentBuilder)`; `install_with` takes the two policy sentences a
candidate replaces at the factory. The App Server production library does not
depend on or install this crate; an embedding host chooses it explicitly.
Settings and Timeline remain existing runtime and App Server owners, consumed
through their public Rust service bindings.

The process row registers Goal's Settings source and two parts. The Session
part registers `session.goal.set`, `edit`, `pause`, `resume` and `clear`,
contributes the `goal_update` tool, and adds the guide entry. The branch part
folds the branch's published facts, decides continuation at a turn end, creates
and ends the evaluator line, and decides the evaluator's report.

Inputs, outputs, configuration, the fold and the decisions are defined in the
[Goal design](../../docs/design/plugins/Goal.md) and the
[Pursuit design](../../docs/design/plugins/Pursuit.md). Every model-facing text
is fixed there and pinned byte for byte in the flows. Public flows live in
`tests/`: a part harness with the minimal assembly Goal needs, and a product
harness that serves the default profile with a scripted local model.
