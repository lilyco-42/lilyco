#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Init,
    Running { current: u32, total: u32 },
    Done,
    Error,
}
