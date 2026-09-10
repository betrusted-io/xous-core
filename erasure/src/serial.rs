pub trait SerialInteract {
    fn rx_char(&mut self, c: u8);
    fn process(&mut self);
}
