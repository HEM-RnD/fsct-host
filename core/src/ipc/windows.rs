// Platform-specific IPC transport for Windows using Tokio Named Pipes
// Provides EndpointListenerBuilder, EndpointListener (as Stream), EndpointClient

use anyhow::{anyhow, Context, Result};
use futures::Stream;
use futures::stream;
use std::{mem, ptr};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};

use winapi::shared::winerror::ERROR_SUCCESS;
use winapi::um::accctrl::*;
use winapi::um::aclapi::*;
use winapi::um::minwinbase::{LPTR, SECURITY_ATTRIBUTES};
use winapi::um::securitybaseapi::*;
use winapi::um::winbase::{LocalAlloc, LocalFree};
use winapi::um::winnt::*;

// --- Listener and client ---

pub struct EndpointListener {
    name: String,
}

impl EndpointListener {
    /// Create a named-pipe listener bound to the given name. Access: allow everyone.
    pub async fn from_path(name: String) -> Result<Self> { Ok(EndpointListener { name }) }

    pub fn listen(self) -> Result<impl Stream<Item=Result<NamedPipeServer>> + Send> {
        let name = self.name.clone();
        // Create the first server instance before starting the unfold
        let first = create_server_allow_all(&name)?;
        let s = stream::unfold((name, first), |(name, server)| async move {
            // Wait for client to connect
            let ret = server.connect().await
                .map(|_| server)
                .map_err(|e| anyhow!("failed to connect to named pipe: {}", e));
            // Pre-create next listening instance before yielding
            let next = match create_server_allow_all(&name) {
                Ok(s) => s,
                Err(_e) => return None,
            };
            Some((ret, (name, next)))
        });
        Ok(s)
    }
}

struct WinSecurityAttributes {
    sid: PSID,
    acl_ptr: PACL,
    sd_ptr: PSECURITY_DESCRIPTOR,
    sa: SECURITY_ATTRIBUTES,
}

impl Drop for WinSecurityAttributes {
    fn drop(&mut self) {
        unsafe {
            if !self.acl_ptr.is_null() { let _ = LocalFree(self.acl_ptr as *mut _); }
            if !self.sd_ptr.is_null() { let _ = LocalFree(self.sd_ptr); }
            if !self.sid.is_null() { FreeSid(self.sid); }
        }
    }
}

impl WinSecurityAttributes {
    fn create_allow_all() -> Result<Self> {
        unsafe {
            // Create empty security attributes to be filled in below. Drop will free them
            // making sure we don't leak memory.
            let mut security_attributes = WinSecurityAttributes {
                sid: ptr::null_mut(),
                acl_ptr: ptr::null_mut(),
                sd_ptr: ptr::null_mut(),
                sa: mem::zeroed(),
            };

            // 1) Create Everyone SID
            // Build a local SID_IDENTIFIER_AUTHORITY for the World authority to avoid mutating a const
            let mut world_auth: SID_IDENTIFIER_AUTHORITY = SID_IDENTIFIER_AUTHORITY { Value: SECURITY_WORLD_SID_AUTHORITY };
            let sid_ok = AllocateAndInitializeSid(
                &mut world_auth as *mut SID_IDENTIFIER_AUTHORITY,
                1,
                SECURITY_WORLD_RID,
                0, 0, 0, 0, 0, 0, 0,
                &mut security_attributes.sid as *mut _,
            );
            if sid_ok == 0 {
                return Err(anyhow!("failed to build Everyone SID: {}", std::io::Error::last_os_error()));
            }

            // 2) Create EXPLICIT_ACCESS for Everyone allowing read/write
            let mut ea: EXPLICIT_ACCESS_W = mem::zeroed();
            ea.Trustee.TrusteeForm = TRUSTEE_IS_SID;
            ea.Trustee.TrusteeType = TRUSTEE_IS_WELL_KNOWN_GROUP;
            ea.Trustee.ptstrName = security_attributes.sid as *mut _;
            ea.grfAccessMode = SET_ACCESS;
            ea.grfAccessPermissions = GENERIC_READ | FILE_WRITE_DATA; // inbound/outbound
            ea.grfInheritance = 0;

            let mut entries = [ea];

            // 3) Create ACL from the single ACE directly
            let result = SetEntriesInAclW(
                entries.len() as u32,
                entries.as_mut_ptr() as *mut _,
                ptr::null_mut(),
                &mut security_attributes.acl_ptr as *mut _,
            );
            if result != ERROR_SUCCESS {
                let err = anyhow!("failed to create ACL: os error {}", result);
                return Err(err);
            }

            // 4) Create and initialize security descriptor, set DACL
            security_attributes.sd_ptr = LocalAlloc(LPTR, SECURITY_DESCRIPTOR_MIN_LENGTH);
            if security_attributes.sd_ptr.is_null() {
                let err = anyhow!("failed to allocate security descriptor");
                return Err(err);
            }

            if InitializeSecurityDescriptor(security_attributes.sd_ptr, SECURITY_DESCRIPTOR_REVISION) == 0 {
                let err = anyhow!("failed to init security descriptor: {}", std::io::Error::last_os_error());
                return Err(err);
            }
            if SetSecurityDescriptorDacl(security_attributes.sd_ptr, 1, security_attributes.acl_ptr, 0) == 0 {
                let err = anyhow!("failed to set DACL: {}", std::io::Error::last_os_error());
                return Err(err);
            }

            // 5) Prepare SECURITY_ATTRIBUTES pointing to our descriptor
            security_attributes.sa.nLength = mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
            security_attributes.sa.lpSecurityDescriptor = security_attributes.sd_ptr;
            security_attributes.sa.bInheritHandle = 0;
            Ok(security_attributes)
        }
    }

    fn as_mut_raw(&mut self) -> *mut SECURITY_ATTRIBUTES {
        &mut self.sa as *mut _
    }
}


// struct WinSecurityAttributesBuilder {}

fn create_server_allow_all(name: &str) -> Result<NamedPipeServer> {
    // Build permissive security attributes (allow everyone) and create the server in this single function.
    unsafe {
        let mut security_attributes = WinSecurityAttributes::create_allow_all()?;

        // 6) Create server with these attributes
        let create_res = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .access_inbound(true)
            .access_outbound(true)
            .in_buffer_size(65536)
            .out_buffer_size(65536)
            .create_with_security_attributes_raw(name, security_attributes.as_mut_raw() as *mut _);
        match create_res {
            Ok(server) => {
                // Cleanup temporary allocations; pipe has copied/consumed security
                Ok(server)
            }
            Err(e) => {
                let err = anyhow!("failed to create named pipe server for {}: {}", name, e);
                Err(err)
            }
        }
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
