//! Where the launcher opens (LA5): the focused output, which is the first output the activated
//! window entered, or the first output when no window is activated there.

/// `activated` is the outputs of the activated window, as the compositor client lists them;
/// `outputs` the connectors of the launcher's surfaces, in their order.
pub fn focused_output(activated: Option<&[String]>, outputs: &[Option<String>]) -> usize {
    activated
        .and_then(|entered| entered.first())
        .and_then(|first| outputs.iter().position(|output| output.as_deref() == Some(first.as_str())))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connectors(names: &[&str]) -> Vec<Option<String>> {
        names.iter().map(|name| Some((*name).to_owned())).collect()
    }

    #[test]
    fn focused_output_follows_the_activated_window() {
        let outputs = connectors(&["DP-1", "HDMI-A-1"]);
        assert_eq!(focused_output(Some(&["HDMI-A-1".to_owned()]), &outputs), 1);
        assert_eq!(focused_output(Some(&["DP-1".to_owned(), "HDMI-A-1".to_owned()]), &outputs), 0, "the first output it entered");
    }

    #[test]
    fn without_an_activated_window_on_a_listed_output_it_is_the_first() {
        let outputs = connectors(&["DP-1", "HDMI-A-1"]);
        assert_eq!(focused_output(None, &outputs), 0);
        assert_eq!(focused_output(Some(&[]), &outputs), 0);
        assert_eq!(focused_output(Some(&["DP-9".to_owned()]), &outputs), 0, "an output that left");
        assert_eq!(focused_output(Some(&["DP-1".to_owned()]), &[None, Some("DP-1".to_owned())]), 1, "an unnamed output never matches");
    }
}
