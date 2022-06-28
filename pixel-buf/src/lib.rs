use dbuf::interface::DefaultOwned;

type DefaultStrategy = dbuf::strategy::HazardStrategy;

pub struct PixelBuf<
    D: Dim,
    S: DefaultOwned<dbuf::raw::SizedRawDoubleBuffer<<D as Dim>::ByteBuf>> = DefaultStrategy,
> {
    buf: dbuf::raw::Writer<
        <S as dbuf::interface::DefaultOwned<
            dbuf::raw::SizedRawDoubleBuffer<<D as Dim>::ByteBuf>,
        >>::StrongRefWithWeak,
    >,
    dim: D,
}

pub unsafe trait Dim: Copy {
    type ByteBuf: AsRef<[u8]> + AsMut<[u8]>;

    fn zeroed(&self) -> Self::ByteBuf;

    fn width(&self) -> u32;
    fn height(&self) -> u32;

    fn len(&self) -> Option<usize> {
        Some(usize::try_from(self.height()).ok()? * usize::try_from(self.width()).ok()?)
    }

    fn index_of(&self, w: u32, h: u32) -> usize {
        debug_assert!(w < self.width());
        debug_assert!(h < self.height());
        debug_assert!(self.len().is_some());
        w as usize * self.height() as usize + h as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Const<const WIDTH: usize, const HEIGHT: usize>;
#[repr(transparent)]
pub struct ConstByteBuf<const WIDTH: usize, const HEIGHT: usize>([[[u8; 4]; WIDTH]; HEIGHT]);

impl<const WIDTH: usize, const HEIGHT: usize> ConstByteBuf<WIDTH, HEIGHT> {
    const LEN: usize = WIDTH * HEIGHT * 4;
    const ZERO: Self = Self([[[0; 4]; WIDTH]; HEIGHT]);
}

impl<const WIDTH: usize, const HEIGHT: usize> AsRef<[u8]> for ConstByteBuf<WIDTH, HEIGHT> {
    fn as_ref(&self) -> &[u8] {
        unsafe { &*core::ptr::slice_from_raw_parts(self as *const Self as *const u8, Self::LEN) }
    }
}

impl<const WIDTH: usize, const HEIGHT: usize> AsMut<[u8]> for ConstByteBuf<WIDTH, HEIGHT> {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe {
            &mut *core::ptr::slice_from_raw_parts_mut(self as *mut Self as *mut u8, Self::LEN)
        }
    }
}

unsafe impl<const WIDTH: usize, const HEIGHT: usize> Dim for Const<WIDTH, HEIGHT> {
    type ByteBuf = ConstByteBuf<WIDTH, HEIGHT>;

    fn zeroed(&self) -> Self::ByteBuf {
        ConstByteBuf::<WIDTH, HEIGHT>::ZERO
    }

    fn width(&self) -> u32 {
        assert!(WIDTH <= u32::MAX as usize);
        WIDTH as u32
    }

    fn height(&self) -> u32 {
        assert!(HEIGHT <= u32::MAX as usize);
        HEIGHT as u32
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Dynamic {
    pub width: u32,
    pub height: u32,
}

unsafe impl Dim for Dynamic {
    type ByteBuf = Vec<u8>;

    fn zeroed(&self) -> Self::ByteBuf {
        let len = usize::try_from(self.width)
            .and_then(|width| Ok((width, usize::try_from(self.height)?)))
            .ok()
            .and_then(|(width, height)| width.checked_mul(height))
            .and_then(|len| len.checked_mul(4))
            .expect("Cannot overflow");
        vec![0; len]
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }
}

pub fn const_sized<const WIDTH: usize, const HEIGHT: usize>() -> PixelBuf<Const<WIDTH, HEIGHT>> {
    PixelBuf {
        dim: Const,
        buf: dbuf::raw::Writer::new(dbuf::ptrs::alloc::OwnedWithWeak::new(
            dbuf::raw::Shared::from_raw_parts(
                DefaultStrategy::default(),
                dbuf::raw::SizedRawDoubleBuffer::new(Const.zeroed(), Const.zeroed()),
            ),
        )),
    }
}

impl<D: Dim, S: DefaultOwned<dbuf::raw::SizedRawDoubleBuffer<<D as Dim>::ByteBuf>>> PixelBuf<D, S> {
    pub fn from_raw_parts(dim: D, strategy: S) -> Self {
        Self {
            buf: dbuf::raw::Writer::new(strategy.build_with_weak(
                dbuf::raw::SizedRawDoubleBuffer::new(dim.zeroed(), dim.zeroed()),
            )),
            dim,
        }
    }

    pub fn read_buf(&self) -> &[u8] {
        self.buf.split().reader.as_ref()
    }

    pub fn write_buf(&self) -> &[u8] {
        self.buf.split().writer.as_ref()
    }

    pub fn write_buf_mut(&mut self) -> &mut [u8] {
        self.buf.split_mut().writer.as_mut()
    }

    pub fn split(&mut self) -> (&mut [u8], &[u8]) {
        let split = self.buf.split_mut();
        (split.writer.as_mut(), split.reader.as_ref())
    }

    pub fn dim(&self) -> D {
        self.dim
    }

    pub fn get(&self, w: u32, h: u32) -> [u8; 4] {
        let index = self.dim.index_of(w, h);
        let pixel = &self.write_buf()[index * 4..][..4];
        pixel.try_into().unwrap()
    }

    pub fn get_mut(&mut self, w: u32, h: u32) -> &mut [u8; 4] {
        let index = self.dim.index_of(w, h);
        let pixel = &mut self.write_buf_mut()[index * 4..][..4];
        pixel.try_into().unwrap()
    }
}
