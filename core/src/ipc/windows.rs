// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.
//
// Portions of this file are derived from parity-tokio-ipc (https://github.com/paritytech/parity-tokio-ipc)) crate,
// Copyright 2017 Nikolay Volf, licensed under Apache License 2.0

// Platform-specific IPC transport for Windows using Tokio Named Pipes
// Provides EndpointListener (as Stream) and EndpointClient

use anyhow::{anyhow, Result};
use futures::Stream;
use futures::stream;
use std::{io, marker, mem, ptr};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use winapi::shared::winerror::{ERROR_SUCCESS};
use winapi::um::accctrl::*;
use winapi::um::aclapi::*;
use winapi::um::minwinbase::{LPTR, PSECURITY_ATTRIBUTES, SECURITY_ATTRIBUTES};
use winapi::um::securitybaseapi::*;
use winapi::um::winbase::{LocalAlloc, LocalFree};
use winapi::um::winnt::*;

pub struct EndpointListener {
    name: String,
    security_attributes: SecurityAttributes,
    pipe_created: bool,
}

impl EndpointListener {
    /// Create a named-pipe listener bound to the given name. Access: allow everyone.
    pub async fn from_path(name: String) -> Result<Self> {
        let security_attributes = SecurityAttributes::allow_everyone_create()?;
        Ok(EndpointListener { name, security_attributes, pipe_created: false })
    }

    pub fn listen(mut self) -> Result<impl Stream<Item=Result<NamedPipeServer>> + Send> {
        // Create the first server instance before starting the unfold
        let first = self.create_server()?;
        let s = stream::unfold((self, first), |(mut endpoint_listener, server)| async move {
            // Wait for client to connect
            let ret = server.connect().await
                .map(|_| server)
                .map_err(|e| anyhow!("failed to connect to named pipe: {}", e));
            // Pre-create next listening instance before yielding
            let next = match endpoint_listener.create_server() {
                Ok(s) => s,
                Err(_e) => return None,
            };
            Some((ret, (endpoint_listener, next)))
        });
        Ok(s)
    }

    fn create_server(&mut self) -> Result<NamedPipeServer> {
        unsafe {
            // 6) Create server with these attributes
            let create_res = ServerOptions::new()
                .first_pipe_instance(!self.pipe_created)
                .reject_remote_clients(true)
                .access_inbound(true)
                .access_outbound(true)
                .in_buffer_size(65536)
                .out_buffer_size(65536)
                .create_with_security_attributes_raw(self.name.as_str(), self.security_attributes.as_ptr() as *mut _);
            match create_res {
                Ok(server) => {
                    self.pipe_created = true;
                    Ok(server)
                }
                Err(e) => {
                    let err = anyhow!("failed to create named pipe server for {}: {}", self.name, e);
                    Err(err)
                }
            }
        }
    }
}

/// Security attributes.
pub struct SecurityAttributes {
    attributes: Option<InnerAttributes>,
}

pub const DEFAULT_SECURITY_ATTRIBUTES: SecurityAttributes = SecurityAttributes {
    attributes: Some(InnerAttributes {
        descriptor: SecurityDescriptor {
            descriptor_ptr: ptr::null_mut(),
        },
        acl: Acl {
            acl_ptr: ptr::null_mut(),
        },
        attrs: SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 0,
        },
    }),
};

impl SecurityAttributes {
    /// New default security attributes.
    pub fn empty() -> SecurityAttributes {
        DEFAULT_SECURITY_ATTRIBUTES
    }

    /// New default security attributes that allow everyone to connect.
    pub fn allow_everyone_connect(&self) -> io::Result<SecurityAttributes> {
        let attributes = Some(InnerAttributes::allow_everyone(
            GENERIC_READ | FILE_WRITE_DATA,
        )?);
        Ok(SecurityAttributes { attributes })
    }

    /// Set a custom permission on the socket
    pub fn set_mode(self, _mode: u32) -> io::Result<Self> {
        // for now, does nothing.
        Ok(self)
    }

    /// New default security attributes that allow everyone to create.
    pub fn allow_everyone_create() -> io::Result<SecurityAttributes> {
        let attributes = Some(InnerAttributes::allow_everyone(
            GENERIC_READ | GENERIC_WRITE,
        )?);
        Ok(SecurityAttributes { attributes })
    }

    /// Return raw handle of security attributes.
    pub(crate) fn as_ptr(&mut self) -> PSECURITY_ATTRIBUTES {
        match self.attributes.as_mut() {
            Some(attributes) => unsafe {attributes.as_ptr()},
            None => ptr::null_mut(),
        }
    }
}

unsafe impl Send for SecurityAttributes {}

struct Sid {
    sid_ptr: PSID,
}

impl Sid {
    fn everyone_sid() -> io::Result<Sid> {
        let mut sid_ptr = ptr::null_mut();
        let result = unsafe {
            #[allow(const_item_mutation)]
            AllocateAndInitializeSid(
                SECURITY_WORLD_SID_AUTHORITY.as_mut_ptr() as *mut _,
                1,
                SECURITY_WORLD_RID,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut sid_ptr,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Sid { sid_ptr })
        }
    }

    // Unsafe - the returned pointer is only valid for the lifetime of self.
    unsafe fn as_ptr(&self) -> PSID {
        self.sid_ptr
    }
}

impl Drop for Sid {
    fn drop(&mut self) {
        if !self.sid_ptr.is_null() {
            unsafe {
                FreeSid(self.sid_ptr);
            }
        }
    }
}

struct AceWithSid<'a> {
    explicit_access: EXPLICIT_ACCESS_W,
    _marker: marker::PhantomData<&'a Sid>,
}

impl<'a> AceWithSid<'a> {
    fn new(sid: &'a Sid, trustee_type: u32) -> AceWithSid<'a> {
        let mut explicit_access = unsafe { mem::zeroed::<EXPLICIT_ACCESS_W>() };
        explicit_access.Trustee.TrusteeForm = TRUSTEE_IS_SID;
        explicit_access.Trustee.TrusteeType = trustee_type;
        explicit_access.Trustee.ptstrName = unsafe { sid.as_ptr() as *mut _ };

        AceWithSid {
            explicit_access,
            _marker: marker::PhantomData,
        }
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
    fn empty() -> io::Result<Acl> {
        Self::new(&mut [])
    }

    fn new(entries: &mut [AceWithSid<'_>]) -> io::Result<Acl> {
        let mut acl_ptr = ptr::null_mut();
        let result = unsafe {
            SetEntriesInAclW(
                entries.len() as u32,
                entries.as_mut_ptr() as *mut _,
                ptr::null_mut(),
                &mut acl_ptr,
            )
        };

        if result != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(result as i32));
        }

        Ok(Acl { acl_ptr })
    }

    unsafe fn as_ptr(&self) -> PACL {
        self.acl_ptr
    }
}

impl Drop for Acl {
    fn drop(&mut self) {
        if !self.acl_ptr.is_null() {
            unsafe { LocalFree(self.acl_ptr as *mut _) };
        }
    }
}

struct SecurityDescriptor {
    descriptor_ptr: PSECURITY_DESCRIPTOR,
}

impl SecurityDescriptor {
    fn new() -> io::Result<Self> {
        let descriptor_ptr = unsafe { LocalAlloc(LPTR, SECURITY_DESCRIPTOR_MIN_LENGTH) };
        if descriptor_ptr.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Failed to allocate security descriptor",
            ));
        }

        if unsafe {
            InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION) == 0
        } {
            return Err(io::Error::last_os_error());
        };

        Ok(SecurityDescriptor { descriptor_ptr })
    }

    fn set_dacl(&mut self, acl: &Acl) -> io::Result<()> {
        if unsafe {
            SetSecurityDescriptorDacl(self.descriptor_ptr, true as i32, acl.as_ptr(), false as i32)
                == 0
        } {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    unsafe fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        self.descriptor_ptr
    }
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
    fn empty() -> io::Result<InnerAttributes> {
        let descriptor = SecurityDescriptor::new()?;
        let mut attrs = unsafe { mem::zeroed::<SECURITY_ATTRIBUTES>() };
        attrs.nLength = mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
        attrs.lpSecurityDescriptor = unsafe { descriptor.as_ptr() };
        attrs.bInheritHandle = false as i32;

        let acl = Acl::empty().expect("this should never fail");

        Ok(InnerAttributes {
            acl,
            descriptor,
            attrs,
        })
    }

    fn allow_everyone(permissions: u32) -> io::Result<InnerAttributes> {
        let mut attributes = Self::empty()?;
        let sid = Sid::everyone_sid()?;

        let mut everyone_ace = AceWithSid::new(&sid, TRUSTEE_IS_WELL_KNOWN_GROUP);
        everyone_ace
            .set_access_mode(SET_ACCESS)
            .set_access_permissions(permissions)
            .allow_inheritance(false as u32);

        let mut entries = vec![everyone_ace];
        attributes.acl = Acl::new(&mut entries)?;
        attributes.descriptor.set_dacl(&attributes.acl)?;

        Ok(attributes)
    }

    unsafe fn as_ptr(&mut self) -> PSECURITY_ATTRIBUTES {
        &mut self.attrs as *mut _
    }
}

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
