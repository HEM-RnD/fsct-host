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

use block2::RcBlock;
use core_foundation_sys::base::TCFTypeRef;
use core_foundation_sys::string::CFStringRef;
use core_foundation_sys::{
    base::{CFRelease, kCFAllocatorDefault},
    bundle::CFBundleCreate,
    bundle::CFBundleRef,
    string::CFStringCreateWithCString,
    url::CFURLCreateWithFileSystemPath,
};
use dispatch2::ffi::dispatch_queue_t;
use dispatch2::{Queue, QueueAttribute};
use libc::{c_char, c_void};
use objc2::Encoding;
use objc2::rc::Retained;
use objc2_foundation::{NSDate, NSDictionary, NSNumber, NSObject, NSString};
use std::any::Any;
use std::collections::HashMap;
use std::mem::transmute;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use anyhow::{anyhow, bail};

/// ObjectiveC declarations:
/// typedef void (^MRMediaRemoteGetNowPlayingInfoCompletion)(CFDictionaryRef information);
/// typedef void (^MRMediaRemoteGetNowPlayingApplicationPIDCompletion)(int PID);
/// typedef void (^MRMediaRemoteGetNowPlayingApplicationIsPlayingCompletion)(Boolean isPlaying);
///
/// void MRMediaRemoteGetNowPlayingApplicationPID(dispatch_queue_t queue, MRMediaRemoteGetNowPlayingApplicationPIDCompletion completion);
/// void MRMediaRemoteGetNowPlayingInfo(dispatch_queue_t queue, MRMediaRemoteGetNowPlayingInfoCompletion completion);
/// void MRMediaRemoteGetNowPlayingApplicationIsPlaying(dispatch_queue_t queue, MRMediaRemoteGetNowPlayingApplicationIsPlayingCompletion completion);
///
/// void MRMediaRemoteRegisterForNowPlayingNotifications(dispatch_queue_t queue);
/// void MRMediaRemoteUnregisterForNowPlayingNotifications();
///
/// usage:
/// MRMediaRemoteGetNowPlayingInfo(dispatch_get_main_queue(), ^(CFDictionaryRef information) {
///         NSLog(@"We got the information: %@", information);
/// });
type MRMediaRemoteGetNowPlayingInfoFn = unsafe extern "C" fn(queue: dispatch_queue_t, completion: *mut c_void);
type MRMediaRemoteGetNowPlayingApplicationPIDFn =
unsafe extern "C" fn(queue: dispatch_queue_t, completion: *mut c_void);
type MRMediaRemoteGetNowPlayingApplicationIsPlayingFn =
unsafe extern "C" fn(queue: dispatch_queue_t, completion: *mut c_void);

type MRMediaRemoteRegisterForNowPlayingNotificationsFn = unsafe extern "C" fn(queue: dispatch_queue_t);
type MRMediaRemoteUnregisterForNowPlayingNotificationsFn = unsafe extern "C" fn();

#[allow(dead_code)]
pub struct MediaRemoteFramework {
    bundle_ref: CFBundleRef,
    queue: Queue,
    get_now_playing_info_fn: MRMediaRemoteGetNowPlayingInfoFn,
    get_now_playing_application_pid_fn: MRMediaRemoteGetNowPlayingApplicationPIDFn,
    get_now_playing_application_is_playing_fn: MRMediaRemoteGetNowPlayingApplicationIsPlayingFn,
    register_for_now_playing_notifications_fn: MRMediaRemoteRegisterForNowPlayingNotificationsFn,
    unregister_for_now_playing_notifications_fn: MRMediaRemoteUnregisterForNowPlayingNotificationsFn,
}

fn to_cfstring(s: &str) -> anyhow::Result<CFStringRef> {
    unsafe {
        let cf_string = CFStringCreateWithCString(
            kCFAllocatorDefault,
            s.as_ptr() as *const i8,
            core_foundation_sys::string::kCFStringEncodingUTF8,
        );
        if cf_string.is_null() {
            return Err(anyhow!("Can't create CFString for {}", s));
        }
        Ok(cf_string)
    }
}

#[allow(non_snake_case)]
fn load_using_cfbundle() -> anyhow::Result<CFBundleRef> {
    unsafe {
        let c_path = "/System/Library/PrivateFrameworks/MediaRemote.framework\0";

        let cf_string_path = to_cfstring(c_path)?;

        let cf_url = CFURLCreateWithFileSystemPath(
            kCFAllocatorDefault,
            cf_string_path,
            core_foundation_sys::url::kCFURLPOSIXPathStyle,
            true as u8,
        );
        CFRelease(cf_string_path.as_void_ptr());

        if cf_url.is_null() {
            bail!("CFURL for the framework path was not created");
        }

        let bundle_ref = CFBundleCreate(kCFAllocatorDefault, cf_url);
        CFRelease(cf_url.as_void_ptr());

        if bundle_ref.is_null() {
            bail!("Failed to load MediaRemote.framework as CFBundle");
        }

        Ok(bundle_ref)
    }
}

unsafe impl Send for MediaRemoteFramework {}
unsafe impl Sync for MediaRemoteFramework {}

struct Desync<T>(T);
unsafe impl<T> Send for Desync<T> {}
unsafe impl<T> Sync for Desync<T> {}

unsafe fn load_function(bundle_ref: CFBundleRef, fn_name: &str) -> anyhow::Result<*const c_void> {
    let fn_pointer = unsafe {
        let fn_name_cfstring = to_cfstring(fn_name)?;
        let fn_pointer = core_foundation_sys::bundle::CFBundleGetFunctionPointerForName(bundle_ref, fn_name_cfstring);
        CFRelease(fn_name_cfstring.as_void_ptr());
        fn_pointer
    };

    if fn_pointer.is_null() {
        bail!("Failed to get function `{fn_name}` pointer");
    }

    Ok(fn_pointer)
}

impl MediaRemoteFramework {
    pub fn load() -> anyhow::Result<Self> {
        let bundle_ref = load_using_cfbundle()?;

        unsafe {
            let get_now_playing_info_fn: MRMediaRemoteGetNowPlayingInfoFn =
                transmute(load_function(bundle_ref, "MRMediaRemoteGetNowPlayingInfo\0")?);
            let get_now_playing_application_pid_fn: MRMediaRemoteGetNowPlayingApplicationPIDFn =
                transmute(load_function(bundle_ref, "MRMediaRemoteGetNowPlayingApplicationPID\0")?);
            let get_now_playing_application_is_playing_fn: MRMediaRemoteGetNowPlayingApplicationIsPlayingFn = transmute(
                load_function(bundle_ref, "MRMediaRemoteGetNowPlayingApplicationIsPlaying\0")?,
            );
            let register_for_now_playing_notifications_fn: MRMediaRemoteRegisterForNowPlayingNotificationsFn =
                transmute(load_function(
                    bundle_ref,
                    "MRMediaRemoteRegisterForNowPlayingNotifications\0",
                )?);
            let unregister_for_now_playing_notifications_fn: MRMediaRemoteUnregisterForNowPlayingNotificationsFn =
                transmute(load_function(
                    bundle_ref,
                    "MRMediaRemoteUnregisterForNowPlayingNotifications\0",
                )?);

            let mut queue = dispatch2::Queue::new("MediaFrameworkReader", QueueAttribute::Concurrent);

            // this function has to be called before activate, but I haven't figured out what it does
            // register_for_now_playing_notifications_fn(queue.as_raw());
            queue.activate();

            Ok(MediaRemoteFramework {
                bundle_ref,
                queue,
                get_now_playing_info_fn,
                get_now_playing_application_pid_fn,
                get_now_playing_application_is_playing_fn,
                register_for_now_playing_notifications_fn,
                unregister_for_now_playing_notifications_fn,
            })
        }
    }

    pub async fn get_now_playing_info(&self) -> anyhow::Result<HashMap<String, Box<dyn Any + Send>>> {
        let get_now_playing_info_fn = self.get_now_playing_info_fn.clone();
        let queue = Desync(unsafe { self.queue.as_raw() });

        let queue = queue;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));
        {
            let block = block2::RcBlock::new(move |information: *mut NSDictionary<NSString, NSObject>| {
                let dict = unsafe { Retained::retain(information) };
                let map = if let Some(dict) = dict {
                    let map = dict_to_hashmap(&dict);
                    map
                } else {
                    HashMap::new()
                };
                let tx = tx.lock().unwrap().take();
                if let Some(tx) = tx {
                    tx.send(map).unwrap();
                }
            });
            unsafe { get_now_playing_info_fn(queue.0, RcBlock::as_ptr(&block) as *mut c_void) };
        }
        let dict = rx.await?;

        Ok(dict)
    }

    #[allow(dead_code)]
    pub async fn is_playing(&self) -> anyhow::Result<bool> {
        let get_now_playing_application_is_playing_fn = self.get_now_playing_application_is_playing_fn.clone();
        let queue = Desync(unsafe { self.queue.as_raw() });
        let queue = queue;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));
        {
            let block = block2::RcBlock::new(move |is_playing: c_char| {
                let is_playing = is_playing != 0;
                let tx = tx.lock().unwrap().take();
                if let Some(tx) = tx {
                    tx.send(is_playing).unwrap();
                }
            });
            unsafe { get_now_playing_application_is_playing_fn(queue.0, RcBlock::as_ptr(&block) as *mut c_void) };
        }
        let is_playing = rx.await?;
        Ok(is_playing)
    }
}

impl Drop for MediaRemoteFramework {
    fn drop(&mut self) {
        unsafe {
            // (self.unregister_for_now_playing_notifications_fn)();
            CFRelease(self.bundle_ref.as_void_ptr());
        }
    }
}

struct UnknownType;

fn to_any(obj: Retained<NSObject>) -> Box<dyn Any + Send> {
    let obj = match obj.downcast::<NSString>() {
        Ok(obj) => return Box::new(obj.to_string()),
        Err(obj) => obj,
    };
    let obj = match obj.downcast::<NSNumber>() {
        Ok(obj) => {
            let encoding = obj.encoding();
            match encoding {
                Encoding::Char => return Box::new(obj.charValue() as i8),
                Encoding::UChar => return Box::new(obj.unsignedCharValue() as u8),
                Encoding::Short => return Box::new(obj.shortValue() as i16),
                Encoding::UShort => return Box::new(obj.unsignedShortValue() as u16),
                Encoding::Int => return Box::new(obj.intValue() as i32),
                Encoding::UInt => return Box::new(obj.unsignedIntValue() as u32),
                Encoding::Long => return Box::new(obj.longValue() as i64),
                Encoding::ULong => return Box::new(obj.unsignedLongValue() as u64),
                Encoding::LongLong => return Box::new(obj.longLongValue() as i64),
                Encoding::ULongLong => return Box::new(obj.unsignedLongLongValue() as u64),
                Encoding::Float => return Box::new(obj.floatValue() as f32),
                Encoding::Double => return Box::new(obj.doubleValue() as f64),
                _ => unreachable!(),
            }
        }

        Err(obj) => obj,
    };
    let _obj = match obj.downcast::<NSDate>() {
        Ok(obj) => {
            return Box::new(
                std::time::SystemTime::UNIX_EPOCH
                    + core::time::Duration::from_secs_f64(unsafe { obj.timeIntervalSince1970() }),
            );
        }

        Err(obj) => obj,
    };
    Box::new(UnknownType)
}

fn dict_to_hashmap(dict: &NSDictionary<NSString, NSObject>) -> HashMap<String, Box<dyn Any + Send>> {
    let mut map = HashMap::new();
    let keys = dict.allKeys();
    for key in keys.iter() {
        let k = key.to_string();
        if let Some(val) = unsafe { dict.valueForKey(key.deref()) } {
            let value = to_any(val);
            map.insert(k, value);
        } else {
            map.insert(k, Box::new(UnknownType));
        }
    }
    map
}
