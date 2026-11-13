pub(super) struct Bits<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Bits<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    pub fn read(&mut self, count: usize) -> Option<u32> {
        let end = self.position.checked_add(count)?;

        if count > 32 || end > self.data.len().checked_mul(8)? {
            return None;
        }

        let mut value = 0;

        for bit in self.position..end {
            value = (value << 1) | u32::from((self.data[bit / 8] >> (7 - bit % 8)) & 1);
        }

        self.position = end;

        Some(value)
    }

    pub fn remaining(&self) -> usize {
        self.data.len() * 8 - self.position
    }
}
