//! Planar audio buffers and borrowed block views.

/// Owned, planar (channel-major) audio storage: `channels` x `capacity` frames.
///
/// Allocated off the RT thread; the RT thread only ever takes views into it.
#[derive(Clone, Debug)]
pub struct AudioBuffer {
    data: Vec<f32>,
    channels: usize,
    capacity: usize,
}

impl AudioBuffer {
    pub fn new(channels: usize, capacity: usize) -> Self {
        Self {
            data: vec![0.0; channels * capacity],
            channels,
            capacity,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn channel(&self, ch: usize) -> &[f32] {
        &self.data[ch * self.capacity..(ch + 1) * self.capacity]
    }

    pub fn channel_mut(&mut self, ch: usize) -> &mut [f32] {
        &mut self.data[ch * self.capacity..(ch + 1) * self.capacity]
    }

    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

    pub fn block(&self, len: usize) -> AudioBlock<'_> {
        assert!(len <= self.capacity);
        AudioBlock {
            data: &self.data,
            channels: self.channels,
            stride: self.capacity,
            len,
        }
    }

    pub fn block_mut(&mut self, len: usize) -> AudioBlockMut<'_> {
        assert!(len <= self.capacity);
        AudioBlockMut {
            data: &mut self.data,
            channels: self.channels,
            stride: self.capacity,
            len,
        }
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut f32 {
        self.data.as_mut_ptr()
    }
}

/// Read-only view of `channels` x `len` frames.
#[derive(Clone, Copy, Debug)]
pub struct AudioBlock<'a> {
    data: &'a [f32],
    channels: usize,
    stride: usize,
    len: usize,
}

impl<'a> AudioBlock<'a> {
    pub const EMPTY: AudioBlock<'static> = AudioBlock {
        data: &[],
        channels: 0,
        stride: 0,
        len: 0,
    };

    /// # Safety
    /// `ptr` must point to `channels * stride` valid floats that are not
    /// written for the lifetime `'a`, and `len <= stride`.
    pub(crate) unsafe fn from_raw(
        ptr: *const f32,
        channels: usize,
        stride: usize,
        len: usize,
    ) -> Self {
        let data = if channels == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(ptr, channels * stride) }
        };
        Self {
            data,
            channels,
            stride,
            len,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0 || self.channels == 0
    }

    pub fn channel(&self, ch: usize) -> &'a [f32] {
        &self.data[ch * self.stride..ch * self.stride + self.len]
    }

    /// Peak absolute sample value across all channels.
    pub fn peak(&self) -> f32 {
        (0..self.channels)
            .flat_map(|c| self.channel(c).iter())
            .fold(0.0f32, |m, s| m.max(s.abs()))
    }
}

/// Mutable view of `channels` x `len` frames.
#[derive(Debug)]
pub struct AudioBlockMut<'a> {
    data: &'a mut [f32],
    channels: usize,
    stride: usize,
    len: usize,
}

impl<'a> AudioBlockMut<'a> {
    /// # Safety
    /// `ptr` must point to `channels * stride` valid floats with no other
    /// live reference for the lifetime `'a`, and `len <= stride`.
    pub(crate) unsafe fn from_raw(
        ptr: *mut f32,
        channels: usize,
        stride: usize,
        len: usize,
    ) -> Self {
        let data = if channels == 0 {
            &mut [][..]
        } else {
            unsafe { std::slice::from_raw_parts_mut(ptr, channels * stride) }
        };
        Self {
            data,
            channels,
            stride,
            len,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0 || self.channels == 0
    }

    pub fn channel(&self, ch: usize) -> &[f32] {
        &self.data[ch * self.stride..ch * self.stride + self.len]
    }

    pub fn channel_mut(&mut self, ch: usize) -> &mut [f32] {
        &mut self.data[ch * self.stride..ch * self.stride + self.len]
    }

    /// Two distinct channels mutably at once (e.g. a stereo pair).
    pub fn channel_pair_mut(&mut self, a: usize, b: usize) -> (&mut [f32], &mut [f32]) {
        assert!(a < b && b < self.channels);
        let (lo, hi) = self.data.split_at_mut(b * self.stride);
        (
            &mut lo[a * self.stride..a * self.stride + self.len],
            &mut hi[..self.len],
        )
    }

    pub fn as_block(&self) -> AudioBlock<'_> {
        AudioBlock {
            data: self.data,
            channels: self.channels,
            stride: self.stride,
            len: self.len,
        }
    }

    pub fn clear(&mut self) {
        for ch in 0..self.channels {
            self.channel_mut(ch).fill(0.0);
        }
    }

    /// Mix `src * gain` into this block. A mono source is spread across all
    /// channels; otherwise channels are matched up to the smaller count.
    pub fn add_from(&mut self, src: &AudioBlock, gain: f32) {
        let len = self.len.min(src.len());
        if src.channels() == 1 {
            let s = src.channel(0);
            for ch in 0..self.channels {
                for (d, x) in self.channel_mut(ch)[..len].iter_mut().zip(s) {
                    *d += x * gain;
                }
            }
        } else {
            for ch in 0..self.channels.min(src.channels()) {
                for (d, x) in self.channel_mut(ch)[..len].iter_mut().zip(src.channel(ch)) {
                    *d += x * gain;
                }
            }
        }
    }

    pub fn copy_from(&mut self, src: &AudioBlock) {
        self.clear();
        self.add_from(src, 1.0);
    }

    pub fn apply_gain(&mut self, gain: f32) {
        for ch in 0..self.channels {
            self.channel_mut(ch).iter_mut().for_each(|s| *s *= gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_spreads_to_stereo() {
        let mut mono = AudioBuffer::new(1, 8);
        mono.channel_mut(0).fill(0.5);
        let mut st = AudioBuffer::new(2, 8);
        st.block_mut(4).add_from(&mono.block(4), 2.0);
        assert_eq!(&st.channel(0)[..5], &[1.0, 1.0, 1.0, 1.0, 0.0]);
        assert_eq!(&st.channel(1)[..4], &[1.0; 4]);
    }

    #[test]
    fn channel_pair() {
        let mut b = AudioBuffer::new(2, 4);
        let mut blk = b.block_mut(4);
        let (l, r) = blk.channel_pair_mut(0, 1);
        l[0] = 1.0;
        r[0] = 2.0;
        assert_eq!(b.channel(1)[0], 2.0);
    }
}
