use calloop::channel::Sender;
use log::info;

const BUS_NAME: &str = "com.feralinteractive.GameMode";
const OBJECT_PATH: &str = "/com/feralinteractive/GameMode";
const CLIENT_COUNT_PROP_NAME: &str = "ClientCount";

/// Watch gamemode daemon state
pub fn watch(sender: Sender<bool>) {
    std::thread::spawn(move || {
        if let Err(e) = run(&sender) {
            info!("GameMode D-Bus watcher unavailable, gamemode pausing disabled: {e}");
        }
    });
}

fn run(sender: &Sender<bool>) -> zbus::Result<()> {
    let conn = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(&conn, BUS_NAME, OBJECT_PATH, BUS_NAME)?;

    // check and send initial state
    let initial: i32 = proxy.get_property(CLIENT_COUNT_PROP_NAME)?;
    let _ = sender.send(initial > 0);

    // Iterate through delta
    for changed in proxy.receive_property_changed::<i32>(CLIENT_COUNT_PROP_NAME) {
        let Ok(count) = changed.get() else {
            // Failure is unrelated, just continue iterating
            continue;
        };

        // Fails once the live-paper gone
        if sender.send(count > 0).is_err() {
            break;
        }
    }
    Ok(())
}
