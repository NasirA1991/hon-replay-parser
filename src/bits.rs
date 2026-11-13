//! LSB-first bit reader matching the K2 engine's `CBufferBit`.

#[derive(Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }
    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }
    #[inline]
    pub fn len_bits(&self) -> usize {
        self.data.len() * 8
    }
    #[inline]
    pub fn remaining(&self) -> usize {
        self.len_bits().saturating_sub(self.pos)
    }
    /// True when a read went past the end of the buffer.
    #[inline]
    pub fn overrun(&self) -> bool {
        self.pos > self.len_bits()
    }
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    #[inline]
    pub fn bit(&mut self) -> bool {
        let byte = self.pos >> 3;
        let v = if byte < self.data.len() {
            (self.data[byte] >> (self.pos & 7)) & 1 != 0
        } else {
            false
        };
        self.pos += 1;
        v
    }

    /// Read up to 64 bits, least significant bit first.
    #[inline]
    pub fn read(&mut self, count: usize) -> u64 {
        debug_assert!(count <= 64);
        if count == 0 {
            return 0;
        }
        let mut out = 0u64;
        let mut got = 0usize;
        while got < count {
            let byte = self.pos >> 3;
            let off = self.pos & 7;
            let avail = 8 - off;
            let take = avail.min(count - got);
            let b = if byte < self.data.len() {
                self.data[byte] as u64
            } else {
                0
            };
            let chunk = (b >> off) & ((1u64 << take) - 1);
            out |= chunk << got;
            got += take;
            self.pos += take;
        }
        out
    }

    pub fn read_bytes(&mut self, count: usize) -> Vec<u8> {
        (0..count).map(|_| self.read(8) as u8).collect()
    }

    /// Read a wide (up to 128-bit) value.
    pub fn read_u128(&mut self, count: usize) -> u128 {
        let lo = self.read(count.min(64)) as u128;
        if count > 64 {
            lo | ((self.read(count - 64) as u128) << 64)
        } else {
            lo
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lsb_first() {
        let d = [0b1010_1100u8, 0xff];
        let mut b = BitReader::new(&d);
        assert_eq!(b.read(2), 0);
        assert_eq!(b.read(3), 0b011);
        assert_eq!(b.read(7), 0b1111_101);
        assert_eq!(b.pos(), 12);
    }
}
