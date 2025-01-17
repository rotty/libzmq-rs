use crate::{error::msg_from_errno, Group, GroupOwned};
use libzmq_sys as sys;
use sys::errno;

use libc::size_t;
use log::error;
use serde::{Deserialize, Serialize};

use std::{
    ffi::{CStr, CString},
    fmt,
    os::raw::c_void,
    ptr, slice,
    str::{self, Utf8Error},
};

/// A generated ID used to route messages to the approriate client.
///
/// A `RoutingId` is an unique temporary identifier for each `Client`
/// connection on a `Server` socket generated by ØMQ.
///
/// # Example
/// ```
/// # use failure::Error;
/// #
/// # fn main() -> Result<(), Error> {
/// use libzmq::{prelude::*, *};
/// use std::convert::TryInto;
///
/// let addr: TcpAddr = "127.0.0.1:*".try_into()?;
///
/// let server = ServerBuilder::new()
///     .bind(addr)
///     .build()?;
///
/// let bound = server.last_endpoint()?;
///
/// let client = ClientBuilder::new()
///     .connect(bound)
///     .build()?;
///
/// // The client initiates the conversation.
/// client.send("")?;
/// // ØMQ generates a `RoutingId` for the client upon reception of the
/// // first message.
/// let msg = server.recv_msg()?;
/// let routing_id = msg.routing_id().unwrap();
///
/// // This `RoutingId` is used to route messages back to the `Client`.
/// let mut msg: Msg = "".into();
/// msg.set_routing_id(routing_id);
/// server.send(msg)?;
/// #
/// #     Ok(())
/// # }
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct RoutingId(u32);

/// A handle to a message owned by ØMQ.
///
/// A ØMQ message is a discrete unit of data passed between applications
/// or components of the same application. ØMQ messages have no internal
/// structure and from the point of view of ØMQ itself they are considered
/// to be opaque binary data.
pub struct Msg {
    msg: sys::zmq_msg_t,
}

impl Msg {
    /// Create an empty `Msg`.
    ///
    /// See [`zmq_msg_init`].
    ///
    /// [`zmq_msg_init`]: http://api.zeromq.org/master:zmq-msg-init
    ///
    /// ```
    /// use libzmq::Msg;
    ///
    /// let msg = Msg::new();
    ///
    /// assert!(msg.is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a `Msg` preallocated with `len` zeroed bytes.
    ///
    /// See [`zmq_msg_init_size`].
    ///
    /// [`zmq_msg_init_size`]: http://api.zeromq.org/master:zmq-msg-init-size
    ///
    /// ```
    /// use libzmq::Msg;
    ///
    /// let size = 420;
    /// let msg = Msg::with_size(size);
    ///
    /// assert_eq!(msg.len(), size);
    /// ```
    pub fn with_size(size: usize) -> Self {
        unsafe {
            Self::deferred_alloc(|msg| {
                sys::zmq_msg_init_size(msg, size as size_t)
            })
        }
    }

    /// Returns the message content size in bytes.
    ///
    /// See [`zmq_msg_size`].
    ///
    /// [`zmq_msg_size`]: http://api.zeromq.org/master:zmq-msg-size
    pub fn len(&self) -> usize {
        unsafe { sys::zmq_msg_size(self.as_ptr()) }
    }

    /// Returns `true` if the message content has size zero.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return the message content as a `str` slice if it is valid UTF-8.
    ///
    /// ```
    /// # use failure::Error;
    /// #
    /// # fn main() -> Result<(), Error> {
    /// use libzmq::Msg;
    ///
    /// let text = "blzit";
    /// let msg = Msg::from(text);
    ///
    /// assert_eq!(msg.to_str()?, text);
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        str::from_utf8(self.as_bytes())
    }

    /// Return the message content as a byte slice.
    ///
    /// ```
    /// use libzmq::Msg;
    ///
    /// let bytes: &[u8] = b"blzit";
    /// let msg = Msg::from(bytes);
    ///
    /// assert_eq!(msg.as_bytes(), bytes);
    /// ```
    pub fn as_bytes<'a>(&self) -> &'a [u8] {
        // This is safe because we're constraining the slice to the lifetime of
        // this message.
        unsafe {
            let ptr = &self.msg as *const _ as *mut _;
            let data = sys::zmq_msg_data(ptr);
            slice::from_raw_parts(data as *mut u8, self.len())
        }
    }

    /// Return the message content as a mutable byte slice.
    pub fn as_bytes_mut<'a>(&mut self) -> &'a mut [u8] {
        // This is safe because we're constraining the slice to the lifetime of
        // this message.
        unsafe {
            let data = sys::zmq_msg_data(self.as_mut_ptr());
            slice::from_raw_parts_mut(data as *mut u8, self.len())
        }
    }

    /// Get routing ID property on the message.
    ///
    /// See [`zmq_msg_routing_id`].
    ///
    /// [`zmq_msg_routing_id`]: http://api.zeromq.org/master:zmq-msg-routing-id
    pub fn routing_id(&self) -> Option<RoutingId> {
        let rc = unsafe {
            // This is safe since `zmq_msg_routing_id` has the wrong signature.
            // The `msg` pointer should be `*const zmq_msg_t` since
            // the it is not modified by the operation.
            let ptr = self.as_ptr() as *mut _;
            sys::zmq_msg_routing_id(ptr)
        };
        if rc == 0 {
            None
        } else {
            Some(RoutingId(rc))
        }
    }

    /// Set routing ID property on the message.
    ///
    /// # Usage Contract
    /// * Cannot be zero
    ///
    /// # Returned Error Variants
    /// * [`InvalidInput`] (if contract is not followed)
    ///
    /// See [`zmq_msg_set_routing_id`].
    ///
    /// [`zmq_msg_set_routing_id`]: http://api.zeromq.org/master:zmq-msg-set-routing-id
    /// [`InvalidInput`]: ../enum.Error.html#variant.InvalidInput
    pub fn set_routing_id(&mut self, routing_id: RoutingId) {
        let rc = unsafe {
            sys::zmq_msg_set_routing_id(self.as_mut_ptr(), routing_id.0)
        };

        // Should never occur.
        if rc != 0 {
            let errno = unsafe { sys::zmq_errno() };
            panic!(msg_from_errno(errno));
        }
    }

    /// The group property on the message.
    pub fn group(&self) -> Option<&Group> {
        // This is safe we don't actually mutate the msg.
        let mut_msg_ptr = self.as_ptr() as *mut _;
        let char_ptr = unsafe { sys::zmq_msg_group(mut_msg_ptr) };

        if char_ptr.is_null() {
            None
        } else {
            let c_str = unsafe { CStr::from_ptr(char_ptr).to_str().unwrap() };
            Some(Group::from_str_unchecked(c_str))
        }
    }

    /// Set the group property on the message.
    ///
    /// ```
    /// # use failure::Error;
    /// #
    /// # fn main() -> Result<(), Error> {
    /// use libzmq::{Msg, Group};
    /// use std::convert::TryInto;
    ///
    /// let a: &Group = "A".try_into()?;
    ///
    /// let mut msg: Msg = "some msg".into();
    /// msg.set_group(a);
    /// assert_eq!(a, msg.group().unwrap());
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// # Usage Contract
    /// * Cannot hold more than 15 characters.
    ///
    /// # Returned Error Variants
    /// * [`InvalidInput`] (if contract is not followed)
    ///
    /// [`InvalidInput`]: ../enum.Error.html#variant.InvalidInput
    pub fn set_group<G>(&mut self, group: G)
    where
        G: Into<GroupOwned>,
    {
        let group = group.into();
        let string: String = group.into();
        let c_string = CString::new(string).unwrap();
        let rc = unsafe {
            sys::zmq_msg_set_group(self.as_mut_ptr(), c_string.as_ptr())
        };

        // Should never occur.
        if rc == -1 {
            let errno = unsafe { sys::zmq_errno() };
            panic!(msg_from_errno(errno));
        }
    }

    // Defers the allocation of a zmq_msg_t to the closure.
    //
    // TODO Consider allocating without zeroing.
    // https://doc.rust-lang.org/std/mem/union.MaybeUninit.html
    unsafe fn deferred_alloc<F>(f: F) -> Msg
    where
        F: FnOnce(&mut sys::zmq_msg_t) -> i32,
    {
        // This calls mem::zeroed().
        let mut msg = sys::zmq_msg_t::default();

        let rc = f(&mut msg);
        if rc == -1 {
            panic!(msg_from_errno(sys::zmq_errno()));
        }

        Msg { msg }
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut sys::zmq_msg_t {
        &mut self.msg
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zmq_msg_t {
        &self.msg
    }

    pub(crate) fn has_more(&self) -> bool {
        let rc = unsafe { sys::zmq_msg_more(self.as_ptr()) };
        rc != 0
    }
}

impl PartialEq for Msg {
    /// Compares the two underlying raw C pointers.
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.as_ptr(), other.as_ptr())
    }
}

impl Eq for Msg {}

impl fmt::Debug for Msg {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.as_bytes())
    }
}

impl Default for Msg {
    /// Initialises an empty ØMQ message.
    ///
    /// See [`zmq_msg_init`].
    ///
    /// [`zmq_msg_init`]: http://api.zeromq.org/master:zmq-msg-init
    fn default() -> Self {
        unsafe { Self::deferred_alloc(|msg| sys::zmq_msg_init(msg)) }
    }
}

impl Clone for Msg {
    /// Copy the content of the message into another message.
    ///
    /// See [`zmq_msg_copy`].
    ///
    /// [`zmq_msg_copy`]: http://api.zeromq.org/master:zmq-msg-copy
    fn clone(&self) -> Self {
        let mut msg = Msg::new();

        let rc = unsafe {
            // This is safe since `zmq_msg_copy` has the wrong signature.
            // The `src_` pointer should be `*const zmq_msg_t` since
            // the source message is not modified by the operation.
            let ptr = self.as_ptr() as *mut _;
            sys::zmq_msg_copy(msg.as_mut_ptr(), ptr)
        };
        if rc != 0 {
            let errno = unsafe { sys::zmq_errno() };

            match errno {
                errno::EFAULT => panic!("invalid message"),
                _ => panic!(msg_from_errno(errno)),
            }
        }

        msg
    }
}

impl Drop for Msg {
    /// Releases the ØMQ message.
    ///
    /// See [`zmq_msg_close`].
    ///
    /// [`zmq_msg_close`]: http://api.zeromq.org/master:zmq-msg-close
    fn drop(&mut self) {
        let rc = unsafe { sys::zmq_msg_close(self.as_mut_ptr()) };

        if rc != 0 {
            let errno = unsafe { sys::zmq_errno() };
            error!("error while dropping message: {}", msg_from_errno(errno));
        }
    }
}

impl From<Box<[u8]>> for Msg {
    /// Converts of box of bytes into a `Msg` without copying.
    fn from(data: Box<[u8]>) -> Msg {
        unsafe extern "C" fn drop_zmq_msg_t(
            data: *mut c_void,
            _hint: *mut c_void,
        ) {
            // Convert the pointer back into a Box and drop it.
            Box::from_raw(data as *mut u8);
        }

        if data.is_empty() {
            return Msg::new();
        }

        let size = data.len() as size_t;
        let data = Box::into_raw(data);

        unsafe {
            Self::deferred_alloc(|msg| {
                sys::zmq_msg_init_data(
                    msg,
                    data as *mut c_void,
                    size,
                    Some(drop_zmq_msg_t),
                    ptr::null_mut(), // hint
                )
            })
        }
    }
}

impl<'a> From<&[u8]> for Msg {
    /// Converts a byte slice into a `Msg` by copying.
    fn from(slice: &[u8]) -> Self {
        unsafe {
            let mut msg = Msg::with_size(slice.len());

            ptr::copy_nonoverlapping(
                slice.as_ptr(),
                msg.as_bytes_mut().as_mut_ptr(),
                slice.len(),
            );

            msg
        }
    }
}

macro_rules! array_impls {
    ($($N:expr)+) => {
        $(
            impl From<[u8; $N]> for Msg {
                /// Converts an array into a `Msg` without copying.
                fn from(array: [u8; $N]) -> Self {
                    let boxed: Box<[u8]> = Box::new(array);
                    Msg::from(boxed)
                }
            }
        )+
    }
}

array_impls! {
         0  1  2  3  4  5  6  7  8  9
        10 11 12 13 14 15 16 17 18 19
        20 21 22 23 24 25 26 27 28 29
        30 31 32
}

impl From<Vec<u8>> for Msg {
    /// Converts a byte vector into a `Msg` without copying.
    fn from(bytes: Vec<u8>) -> Self {
        Msg::from(bytes.into_boxed_slice())
    }
}

impl<'a> From<&'a str> for Msg {
    /// Converts a `str` slice into a `Msg` by copying.
    fn from(text: &str) -> Self {
        Msg::from(text.as_bytes())
    }
}

impl From<String> for Msg {
    /// Converts a `String` into a `Msg` by without copying.
    fn from(text: String) -> Self {
        Msg::from(text.into_bytes())
    }
}

impl<'a, T> From<&'a T> for Msg
where
    T: Into<Msg> + Clone,
{
    fn from(v: &'a T) -> Self {
        v.clone().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::mem;

    #[test]
    fn test_cast_routing_id_slice() {
        assert_eq!(mem::size_of::<u32>(), mem::size_of::<RoutingId>());
        let routing_stack: &[u32] = &[1, 2, 3, 4];

        // Cast &[u32] as &[RoutingId].
        let cast_stack = unsafe {
            slice::from_raw_parts(
                routing_stack.as_ptr() as *const RoutingId,
                routing_stack.len(),
            )
        };

        for (&i, &j) in routing_stack.iter().zip(cast_stack.iter()) {
            assert_eq!(i, j.0);
        }
    }
}
