mod app;
mod render;

use app::App;
use smithay_client_toolkit::reexports::client::{Connection, globals::registry_queue_init};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let mut app = App::new(&globals, &qh)?;

    while !app.exit() {
        event_queue.blocking_dispatch(&mut app)?;
    }
    Ok(())
}
