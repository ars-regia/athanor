//! The running applications of the `bar` preset (doc_bar.md, BR3, BR7), drawn by the row
//! athanor-apps shares with the dock.

use std::rc::Rc;

use athanor_apps::row::Row;

use super::{popup, Bar, Changed, ModuleUi};

struct RunningUi {
    row: Row,
}

impl ModuleUi for RunningUi {
    fn widget(&self) -> gtk4::Widget {
        self.row.widget()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if matches!(changed, Changed::Windows | Changed::Favorites) {
            self.row.refresh(bar);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let row = Row::new(
        bar,
        gtk4::Orientation::Horizontal,
        popup::towards_inside(bar),
    )?;
    Some(Box::new(RunningUi { row }))
}
