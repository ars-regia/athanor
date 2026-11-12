//! Two layer surfaces on the first output, as athanor-osd's will be (doc_osd.md OD11, OD18, OD19):
//! a dialog whose root is an alert dialog, named by its question, holding two radios and the
//! buttons that drive the other surface, and a status that never takes the keyboard, named by
//! its text. The rig's check of Sal's requirement S2 (`toolkit_a11y_check.py`): "Hide status"
//! takes the status out of the accessibility tree, "Show status" brings it back, and
//! "Announce" makes it announce "Volume 50 percent"; "Hide dialog" takes the dialog out, and the
//! status's "Show dialog" brings it back; "Close status" closes the status.

use athanor_sal::driver::{
    self, Anchor, Application, KeyboardInteractivity, Layer, LayerOptions, OutputInfo, Shell,
    SurfaceId,
};
use athanor_sal::{Event, Kind, RootRole, Theme, Ui};
use std::process::ExitCode;

/// The text the status announces.
const ANNOUNCED: &str = "Volume 50 percent";

#[derive(Default)]
struct Osd {
    /// The dialog opened, on the first output.
    opened: bool,
    /// The dialog, once it is on an output.
    dialog: Option<SurfaceId>,
    /// The status surface, once the dialog has opened it.
    status: Option<SurfaceId>,
}

fn options(namespace: &str, anchor: Anchor, keyboard: KeyboardInteractivity) -> LayerOptions {
    let mut options = LayerOptions::new(Layer::Overlay);
    options.namespace = namespace.into();
    options.anchor = anchor;
    options.keyboard = keyboard;
    options
}

impl Application for Osd {
    fn output(&mut self, _: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        if std::mem::replace(&mut self.opened, true) {
            return None;
        }
        let mut ui = Ui::new("What did you plug in?", Theme::default(), false);
        ui.root_role = RootRole::AlertDialog;
        let choices = ui.add(Ui::ROOT, Kind::Column);
        ui.radio(choices, "Headphones", true);
        ui.radio(choices, "Headset with microphone", false);
        for name in [
            "Hide status",
            "Show status",
            "Announce",
            "Hide dialog",
            "Close status",
        ] {
            ui.add(Ui::ROOT, Kind::Button(name.into()));
        }
        let keyboard = KeyboardInteractivity::OnDemand;
        Some((ui, options("athanor-sal-dialog", Anchor::empty(), keyboard)))
    }

    fn output_changed(&mut self, shell: &mut Shell, dialog: SurfaceId, _: &OutputInfo) {
        if self.status.is_some() {
            return;
        }
        self.dialog = Some(dialog);
        let mut ui = Ui::new("Volume 45 percent", Theme::default(), false);
        ui.root_role = RootRole::Status;
        ui.add(Ui::ROOT, Kind::Label("45 %".into()));
        ui.add(Ui::ROOT, Kind::Button("Show dialog".into()));
        let keyboard = KeyboardInteractivity::None;
        let options = options("athanor-sal-status", Anchor::TOP, keyboard);
        match shell.open_layer(ui, options, None) {
            Ok(id) => self.status = Some(id),
            Err(error) => shell.fail(error),
        }
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        let (Event::Clicked(id), Some(dialog), Some(status)) = (event, self.dialog, self.status)
        else {
            return;
        };
        let name = match shell.ui(surface).map(|ui| &ui.widget(id).kind) {
            Some(Kind::Button(name)) => name.clone(),
            _ => return,
        };
        let done = match name.as_str() {
            "Hide status" => shell.set_accessible(status, false),
            "Show status" => shell.set_accessible(status, true),
            "Hide dialog" => shell.set_accessible(dialog, false),
            "Show dialog" => shell.set_accessible(dialog, true),
            "Close status" => {
                shell.close(status);
                Ok(())
            }
            "Announce" => shell
                .ui_mut(status)
                .map(|ui| ui.announce(ANNOUNCED))
                .ok_or_else(|| "the status surface is closed".into()),
            _ => Ok(()),
        };
        match done {
            Ok(()) => println!("{name}"),
            Err(error) => shell.fail(error),
        }
    }
}

fn main() -> ExitCode {
    match driver::run(Osd::default()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("athanor-sal-a11y: {error}");
            ExitCode::FAILURE
        }
    }
}
