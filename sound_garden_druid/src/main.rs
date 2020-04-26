use anyhow::Result;
use druid::{AppLauncher, Widget, WindowDesc};
use std::sync::Arc;
use xi_rope::engine::Engine;

mod canvas;
mod types;

/* Sound Garden Druid
 *
 * SGD is a collaborative interface for Sound Garden's Audio Synth Server and VST plugin.
 * It features modal editing of SG programs suitable for livecoding and supports P2P jams.
 * Eventually it could incorporate visual feedback like oscilloscope.
 *
 * Let's talk about design. SGD consists of
 * - UI, initially mimicking basic terminal, grid of monospace characters.
 * -- This is going to be implemented using druid. We benefit from its widget paint flexibility
 *    and beautiful font rendering. It's widget library and layout engine is less of use for now.
 * - Persistence layer which allows to save and load programs.
 * -- Let's ser/de engine state with serde (see below).
 * - Connectivity module to send programs and commands to synth server (standalone or VST plugin).
 * -- Regular std::net::TCPStream + serde should be enough.
 * - Connectivity module to exchange programs edits with peers.
 * -- Ideally we want some robust fully decentralised P2P protocol but having "LAN-oriented"
 *    naive code paired with TailScale should be enough for the start.
 * - Editing engine to support edits exchange with undo/redo.
 * -- We are going to use xi_rope::Engine for that. We imagine it'd work as follows:
 *    each peer creates a master engine for itself and one engine per connected peer.
 *    When user changes node model is converted into a normalised text representation
 *    of nodes with their metadata. Change in this representation is used to produce a delta
 *    which is sent to other peers. When peer receives delta it applies it to the corresponding
 *    peer engine and them merges it to master engine.
 *    Undo history is local and based on master engine.
 * - Persistent undo/redo.
 * -- Should come for free when persisting engine state.
 *
 * Physically editing screen could look like:
 *
 * /--------------\
 * |  440 s       |
 * |              |
 * |   .2 *       |
 * |              |
 * \--------------/
 *
 * While for editing engine it could be represented as
 * `123:0:1:440 124:0:5:s 125:2:2:.2 126:2:5:*`
 * (other separators or even format as a whole should be considered)
 * and treated as if was edited as such.
 *
 * This is a bit of a hack but should be safe and it would allow us to not invent own CRDT solution.
 */

/* Plan of Attack
 *
 * [ ] First, let's implement basic canvas to render nodes.
 * [ ] Then we can have some basic editing with a master engine only.
 * [ ] ???
 * [ ] Profit!
 */

struct App {
    master_engine: Arc<Engine>,
}

fn main() -> Result<()> {
    AppLauncher::with_window(WindowDesc::new(build_ui)).launch(Default::default())?;
    Ok(())
}

fn build_ui() -> impl Widget<canvas::Data> {
    canvas::Widget::default()
}
