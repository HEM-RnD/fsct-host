// Platform-specific IPC transport for Windows using Tokio Named Pipes
// Provides EndpointListenerBuilder, EndpointListener (as Stream), EndpointClient

use anyhow::{anyhow, Context, Result};
use futures::Stream;
use futures::stream;
use std::{marker, mem, ptr};
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};

use winapi::shared::winerror::ERROR_SUCCESS;
use winapi::um::accctrl::*;
use winapi::um::aclapi::*;
use winapi::um::minwinbase::{LPTR, PSECURITY_ATTRIBUTES, SECURITY_ATTRIBUTES};
use winapi::um::securitybaseapi::*;
use winapi::um::winbase::{LocalAlloc, LocalFree};
use winapi::um::winnt::*;

// --- SecurityAttributes implementation (Windows) ---
pub struct SecurityAttributes {
    attributes: Option<InnerAttributes>,
}

impl Default for SecurityAttributes {
    fn default() -> Self {
        SecurityAttributes { attributes: None }
    }
}

impl Clone for SecurityAttributes {
    fn clone(&self) -> Self {
        // Deep cloning of raw pointers is complex; instead, for cloning we drop to None (default)
        // because attributes are used only for creation. Each listener instance will create fresh attributes when needed.
        SecurityAttributes { attributes: None }
    }
}

impl SecurityAttributes {
    pub fn allow_all() -> Self {
        // Try to build permissive DACL; on failure, fall back to default (None) which uses process default DACL.
        match InnerAttributes::allow_everyone(GENERIC_READ | FILE_WRITE_DATA) {
            Ok(inner) => SecurityAttributes { attributes: Some(inner) },
            Err(_) => SecurityAttributes { attributes: None },
        }
    }

    unsafe fn as_ptr(&mut self) -> PSECURITY_ATTRIBUTES {
        match self.attributes.as_mut() {
            Some(inner) => unsafe { inner.as_ptr() },
            None => ptr::null_mut(),
        }
    }
}

unsafe impl Send for SecurityAttributes {}

struct Sid {
    sid_ptr: PSID,
}
impl Sid {
    fn everyone_sid() -> std::io::Result<Sid> {
        let mut sid_ptr = ptr::null_mut();
        let result = unsafe {
            #[allow(const_item_mutation)]
            AllocateAndInitializeSid(
                SECURITY_WORLD_SID_AUTHORITY.as_mut_ptr() as *mut _,
                1,
                SECURITY_WORLD_RID,
                0, 0, 0, 0, 0, 0, 0,
                &mut sid_ptr,
            )
        };
        if result == 0 { Err(std::io::Error::last_os_error()) } else { Ok(Sid { sid_ptr }) }
    }
    unsafe fn as_ptr(&self) -> PSID { self.sid_ptr }
}
impl Drop for Sid {
    fn drop(&mut self) {
        if !self.sid_ptr.is_null() { unsafe { FreeSid(self.sid_ptr) }; }
    }
}

struct AceWithSid<'a> {
    explicit_access: EXPLICIT_ACCESS_W,
    _marker: marker::PhantomData<&'a Sid>,
}
impl<'a> AceWithSid<'a> {
    fn new(sid: &'a Sid, trustee_type: u32) -> Self {
        let mut explicit_access = unsafe { mem::zeroed::<EXPLICIT_ACCESS_W>() };
        explicit_access.Trustee.TrusteeForm = TRUSTEE_IS_SID;
        explicit_access.Trustee.TrusteeType = trustee_type;
        explicit_access.Trustee.ptstrName = unsafe { sid.as_ptr() as *mut _ };
        Self { explicit_access, _marker: marker::PhantomData }
    }
    fn set_access_mode(&mut self, access_mode: u32) -> &mut Self {
        self.explicit_access.grfAccessMode = access_mode;
        self
    }
    fn set_access_permissions(&mut self, access_permissions: u32) -> &mut Self {
        self.explicit_access.grfAccessPermissions = access_permissions;
        self
    }
    fn allow_inheritance(&mut self, inheritance_flags: u32) -> &mut Self {
        self.explicit_access.grfInheritance = inheritance_flags;
        self
    }
}

struct Acl {
    acl_ptr: PACL,
}
impl Acl {
    fn empty() -> std::io::Result<Acl> { Self::new(&mut []) }
    fn new(entries: &mut [AceWithSid<'_>]) -> std::io::Result<Acl> {
        let mut acl_ptr = ptr::null_mut();
        let result = unsafe {
            SetEntriesInAclW(
                entries.len() as u32,
                entries.as_mut_ptr() as *mut _,
                ptr::null_mut(),
                &mut acl_ptr,
            )
        };
        if result != ERROR_SUCCESS { return Err(std::io::Error::from_raw_os_error(result as i32)); }
        Ok(Acl { acl_ptr })
    }
    unsafe fn as_ptr(&self) -> PACL { self.acl_ptr }
}
impl Drop for Acl {
    fn drop(&mut self) { if !self.acl_ptr.is_null() { unsafe { LocalFree(self.acl_ptr as *mut _) }; } }
}

struct SecurityDescriptor {
    descriptor_ptr: PSECURITY_DESCRIPTOR,
}
impl SecurityDescriptor {
    fn new() -> std::io::Result<Self> {
        let descriptor_ptr = unsafe { LocalAlloc(LPTR, SECURITY_DESCRIPTOR_MIN_LENGTH) };
        if descriptor_ptr.is_null() { return Err(std::io::Error::new(std::io::ErrorKind::Other, "Failed to allocate security descriptor")); }
        if unsafe { InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(SecurityDescriptor { descriptor_ptr })
    }
    fn set_dacl(&mut self, acl: &Acl) -> std::io::Result<()> {
        if unsafe { SetSecurityDescriptorDacl(self.descriptor_ptr, true as i32, acl.as_ptr(), false as i32) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    unsafe fn as_ptr(&self) -> PSECURITY_DESCRIPTOR { self.descriptor_ptr }
}
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.descriptor_ptr.is_null() {
            unsafe { LocalFree(self.descriptor_ptr) };
            self.descriptor_ptr = ptr::null_mut();
        }
    }
}

struct InnerAttributes {
    descriptor: SecurityDescriptor,
    acl: Acl,
    attrs: SECURITY_ATTRIBUTES,
}
impl InnerAttributes {
    fn empty() -> std::io::Result<Self> {
        let descriptor = SecurityDescriptor::new()?;
        let mut attrs = unsafe { mem::zeroed::<SECURITY_ATTRIBUTES>() };
        attrs.nLength = mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
        attrs.lpSecurityDescriptor = unsafe { descriptor.as_ptr() };
        attrs.bInheritHandle = 0;
        let acl = Acl::empty()?;
        Ok(InnerAttributes { descriptor, acl, attrs })
    }
    fn allow_everyone(permissions: u32) -> std::io::Result<Self> {
        let mut attributes = Self::empty()?;
        let sid = Sid::everyone_sid()?;
        let mut everyone_ace = AceWithSid::new(&sid, TRUSTEE_IS_WELL_KNOWN_GROUP);
        everyone_ace.set_access_mode(SET_ACCESS).set_access_permissions(permissions).allow_inheritance(0);
        let mut entries = vec![everyone_ace];
        attributes.acl = Acl::new(&mut entries)?;
        attributes.descriptor.set_dacl(&attributes.acl)?;
        Ok(attributes)
    }
    unsafe fn as_ptr(&mut self) -> PSECURITY_ATTRIBUTES { &mut self.attrs as *mut _ }
}

// --- Listener and client ---

pub struct EndpointListenerBuilder {
    name: String,
    attrs: SecurityAttributes,
}

impl EndpointListenerBuilder {
    pub fn from_path(name: String) -> Self { // name is like \\.:\pipe\fsct...
        Self { name, attrs: SecurityAttributes::allow_all() }
    }
    pub fn security_attributes(mut self, attrs: SecurityAttributes) -> Self {
        self.attrs = attrs;
        self
    }
    pub async fn build(self) -> Result<EndpointListener> {
        // Defer server instance creation to listen() unfold
        Ok(EndpointListener { name: self.name, attrs: self.attrs })
    }
}

fn create_server(name: &str, attrs: &mut SecurityAttributes) -> Result<NamedPipeServer> {
    // If explicit security attributes are present, use them; else fallback to default create()
    let mut sa_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let has_attrs = attrs.attributes.is_some();
    if has_attrs {
        unsafe { sa_ptr = attrs.as_ptr() as *mut std::ffi::c_void; }
    }
    let server = if has_attrs {
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .access_inbound(true)
                .access_outbound(true)
                .in_buffer_size(65536)
                .out_buffer_size(65536)
                .create_with_security_attributes_raw(name, sa_ptr)
        }
            .with_context(|| format!("failed to create named pipe server for {}", name))?
    } else {
        ServerOptions::new()
            .first_pipe_instance(true)
            .create(name)
            .with_context(|| format!("failed to create named pipe server for {}", name))?
    };
    Ok(server)
}

pub struct EndpointListener {
    name: String,
    attrs: SecurityAttributes,
}

impl EndpointListener {
    pub fn listen(self) -> Result<impl Stream<Item=Result<NamedPipeServer>> + Send> {
        let name = self.name.clone();
        let mut attrs = self.attrs; // move, keep mutable across iterations
        // Create the first server instance before starting the unfold, mirroring parity's approach
        let first = create_server(&name, &mut attrs)?;
        let s = stream::unfold((name, attrs, first), |(name, mut attrs, mut server)| async move {
            // Wait for client to connect
            let ret = server.connect().await
                .map(|_| server)
                .map_err(|e| anyhow!("failed to connect to named pipe: {}", e));
            // Pre-create next listening instance before yielding
            let next = match create_server(&name, &mut attrs) {
                Ok(s) => s,
                Err(e) => return None,
            };
            Some((ret, (name, attrs, next)))
        });
        Ok(s)
    }
}

pub struct EndpointClient;

impl EndpointClient {
    pub async fn connect(name: String) -> Result<NamedPipeClient> {
        ClientOptions::new().open(name.as_str()).context("windows pipe connect failed")
    }
}

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
