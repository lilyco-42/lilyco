pub trait GuiBackend {
    type Window;
    type Error;

    fn init_window(title: &str, width: u32, height: u32) -> Result<Self::Window, Self::Error>;
    fn render_frame(window: &mut Self::Window) -> Result<(), Self::Error>;
}

pub struct GenericGuiAdapter;

impl GuiBackend for GenericGuiAdapter {
    type Window = ();
    type Error = &'static str;

    fn init_window(title: &str, _w: u32, _h: u32) -> Result<(), &'static str> {
        if title.is_empty() {
            return Err("Invalid window title");
        }
        Ok(())
    }

    fn render_frame(_window: &mut ()) -> Result<(), &'static str> {
        Ok(())
    }
}
