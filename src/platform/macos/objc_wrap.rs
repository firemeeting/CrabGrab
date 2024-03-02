#![allow(unused)]
#![allow(non_upper_case_globals)]

#[link(name = "ScreenCaptureKit", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreMedia", kind = "framework")]
#[link(name = "CoreVideo", kind = "framework")]
#[link(name = "IOSurface", kind = "framework")]
#[link(name = "System", kind = "dylib")]
#[link(name = "Foundation", kind = "framework")]
#[link(name = "AppKit", kind = "framework")]
#[link(name = "ApplicationServices", kind = "framework")]
#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

use std::{cell::RefCell, ffi::CString, ops::{Add, Mul, Sub}, sync::Arc, time::{Duration, Instant}};

use block::{Block, ConcreteBlock, RcBlock};
use cocoa::{base::NO, foundation::NSData};
use libc::{c_void, strlen};
use objc::{class, declare::MethodImplementation, msg_send, runtime::{objc_copyProtocolList, objc_getProtocol, Class, Object, Protocol, Sel, BOOL}, sel, sel_impl, Encode, Encoding, Message};
use objc2::runtime::Bool;
use mach2::mach_time::{mach_timebase_info, mach_timebase_info_data_t};

use crate::prelude::{AudioSampleRate, StreamCreateError, StreamError, StreamEvent, StreamStopError};

use lazy_static::lazy_static;
use parking_lot::Mutex;

use super::ImplPixelFormat;

type CFTypeRef = *const c_void;
type CFStringRef = CFTypeRef;
type CMSampleBufferRef = CFTypeRef;
type CFAllocatorRef = CFTypeRef;
type CFDictionaryRef = CFTypeRef;
type CMFormatDescriptionRef = CFTypeRef;
type CMBlockBufferRef = CFTypeRef;
type CFArrayRef = CFTypeRef;
type OSStatus = i32;
type CGDisplayStreamRef = CFTypeRef;
type CGDisplayStreamUpdateRef = CFTypeRef;
type IOSurfaceRef = CFTypeRef;
type CGDictionaryRef = CFTypeRef;
type CFBooleanRef = CFTypeRef;
type CFNumberRef = CFTypeRef;

#[allow(unused)]
fn debug_objc_class(name: &str) {
    let class_name_cstring = CString::new(name).unwrap();
    let class = unsafe { &*objc::runtime::objc_getClass(class_name_cstring.as_ptr()) };
    println!("instance methods: ");
    for method in class.instance_methods().iter() {
        print!("METHOD {}::{}(", class.name(), method.name().name());
        for i in 0 .. method.arguments_count() {
            if i + 1 == method.arguments_count() {
                println!("{}) -> {}", method.argument_type(i).unwrap().as_str(), method.return_type().as_str());
            } else {
                print!("{}, ", method.argument_type(i).unwrap().as_str());
            }
        }
    }
    println!("instance variables: ");
    for ivar in class.instance_variables().iter() {
        println!("IVAR {}::{}: {}", class.name(), ivar.name(), ivar.type_encoding().as_str());
    }
    let metaclass = class.metaclass();
    let metaclass_ptr = metaclass as *const _;
    println!("metaclass ptr: {:?}", metaclass_ptr);
    println!("class methods: ");
    for method in metaclass.instance_methods().iter() {
        print!("CLASS METHOD {}::{}(", class.name(), method.name().name());
        for i in 0 .. method.arguments_count() {
            if i + 1 == method.arguments_count() {
                println!("{}) -> {}", method.argument_type(i).unwrap().as_str(), method.return_type().as_str());
            } else {
                print!("{}, ", method.argument_type(i).unwrap().as_str());
            }
        }
    }

    println!("class ivars: ");
    for ivar in metaclass.instance_variables().iter() {
        println!("CLASS IVAR {}::{}: {}", class.name(), ivar.name(), ivar.type_encoding().as_str());
    }

    println!("protocols: ");
    for protocol in class.adopted_protocols().iter() {
        println!("PROTOCOL {}", protocol.name());
    }
    println!("end");
}

pub(crate) fn debug_objc_object(obj: *mut Object) {
    if (obj.is_null()) {
        println!("debug_objc_object: nil");
        return;
    } else {
        println!("debug_objc_object: {:?}", obj);
    }
    unsafe {
        let class_ptr = objc::runtime::object_getClass(obj);
        if class_ptr.is_null() {
            println!(" * class: nil");
            return;
        }
        let class = &*class_ptr;
        println!(" * class: {}", class.name());
        debug_objc_class(class.name());
    }
}

extern "C" {

    static kCFAllocatorNull: CFTypeRef;

    fn CFRetain(x: CFTypeRef) -> CFTypeRef;
    fn CFRelease(x: CFTypeRef);

    pub(crate) static kCFBooleanTrue: CFBooleanRef;
    pub(crate) static kCFBooleanFalse: CFBooleanRef;

    //CFNumberRef CFNumberCreate(CFAllocatorRef allocator, CFNumberType theType, const void *valuePtr);
    fn CFNumberCreate(allocator: CFAllocatorRef, the_type: isize, value_ptr: *const c_void) -> CFNumberRef;

    fn CGColorGetConstantColor(color_name: CFStringRef) -> CGColorRef;

    static kCGColorBlack: CFStringRef;
    static kCGColorWhite: CFStringRef;
    static kCGColorClear: CFStringRef;

    fn CMTimeMake(value: i64, timescale: i32) -> CMTime;
    fn CMTimeMakeWithEpoch(value: i64, timescale: i32, epoch: i64) -> CMTime;
    fn CMTimeMakeWithSeconds(seconds: f64, preferred_timescale: i32) -> CMTime;
    fn CMTimeGetSeconds(time: CMTime) -> f64;

    fn CMTimeAdd(lhs: CMTime, rhs: CMTime) -> CMTime;
    fn CMTimeSubtract(lhs: CMTime, rhs: CMTime) -> CMTime;
    fn CMTimeMultiply(lhs: CMTime, rhs: i32) -> CMTime;
    fn CMTimeMultiplyByFloat64(time: CMTime, multiplier: f64) -> CMTime;
    fn CMTimeMultiplyByRatio(time: CMTime, multiplier: i32, divisor: i32) -> CMTime;
    fn CMTimeConvertScale(time: CMTime, new_timescale: i32, rounding_method: u32) -> CMTime;
    fn CMTimeCompare(time1: CMTime, time2: CMTime) -> i32;

    static kCMTimeInvalid: CMTime;
    static kCMTimeIndefinite: CMTime;
    static kCMTimePositiveInfinity: CMTime;
    static kCMTimeNegativeInfinity: CMTime;
    static kCMTimeZero: CMTime;

    fn CMSampleBufferCreateCopy(allocator: CFAllocatorRef, original: CMSampleBufferRef, new: *mut CMSampleBufferRef) -> OSStatus;
    fn CMSampleBufferIsValid(sbuf: CMSampleBufferRef) -> Bool;
    fn CMSampleBufferGetNumSamples(sbuf: CMSampleBufferRef) -> isize;
    fn CMSampleBufferGetPresentationTimeStamp(sbuf: CMSampleBufferRef) -> CMTime;
    fn CMSampleBufferGetDuration(sbuf: CMSampleBufferRef) -> CMTime;
    fn CMSampleBufferGetFormatDescription(sbuf: CMSampleBufferRef) -> CMFormatDescriptionRef;
    fn CMSampleBufferGetSampleAttachmentsArray(sbuf: CMSampleBufferRef, create_if_necessary: Bool) -> CFArrayRef;

    fn CMFormatDescriptionGetMediaType(fdesc: CMFormatDescriptionRef) -> OSType;
    fn CMAudioFormatDescriptionGetStreamBasicDescription(afdesc: CMFormatDescriptionRef) -> *const AudioStreamBasicDescription;
    // OSStatus CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(CMSampleBufferRef sbuf, size_t *bufferListSizeNeededOut, AudioBufferList *bufferListOut, size_t bufferListSize, CFAllocatorRef blockBufferStructureAllocator, CFAllocatorRef blockBufferBlockAllocator, uint32_t flags, CMBlockBufferRef  _Nullable *blockBufferOut);
    fn CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(sbuf: CMSampleBufferRef, buffer_list_size_needed_out: *mut usize, buffer_list_out: *mut AudioBufferList, buffer_list_size: usize, block_buffer_structure_allocator: CFAllocatorRef, block_buffer_block_allocator: CFAllocatorRef, flags: u32, block_buffer_out: *mut CMBlockBufferRef) -> OSStatus;

    fn CFArrayGetCount(array: CFArrayRef) -> i32;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: i32) -> CFTypeRef;

    fn CFDictionaryGetValue(dict: CFDictionaryRef, value: CFTypeRef) -> CFTypeRef;

    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;

    //CGDisplayStreamRef CGDisplayStreamCreateWithDispatchQueue(CGDirectDisplayID display, size_t outputWidth, size_t outputHeight, int32_t pixelFormat, CFDictionaryRef properties, dispatch_queue_t queue, CGDisplayStreamFrameAvailableHandler handler);
    fn CGDisplayStreamCreateWithDispatchQueue(display_id: u32, output_width: usize, output_height: usize, pixel_format: i32, properties: CFDictionaryRef, dispatch_queue: *mut Object, handler: *const c_void) -> CGDisplayStreamRef;
    fn CGDisplayStreamStart(stream: CGDisplayStreamRef) -> i32;
    fn CGDisplayStreamStop(stream: CGDisplayStreamRef) -> i32;

    fn CGMainDisplayID() -> u32;

    fn CGRectCreateDictionaryRepresentation(rect: CGRect) -> CFDictionaryRef;

    static mut _dispatch_queue_attr_concurrent: c_void;

    fn dispatch_queue_create(label: *const std::ffi::c_char, attr: DispatchQueueAttr) -> DispatchQueue;
    fn dispatch_retain(object: *mut Object);
    fn dispatch_release(object: *mut Object);

    pub(crate) static SCStreamFrameInfoStatus       : CFStringRef;
    pub(crate) static SCStreamFrameInfoDisplayTime  : CFStringRef;
    pub(crate) static SCStreamFrameInfoScaleFactor  : CFStringRef;
    pub(crate) static SCStreamFrameInfoContentScale : CFStringRef;
    pub(crate) static SCStreamFrameInfoContentRect  : CFStringRef;
    pub(crate) static SCStreamFrameInfoBoundingRect : CFStringRef;
    pub(crate) static SCStreamFrameInfoScreenRect   : CFStringRef;
    pub(crate) static SCStreamFrameInfoDirtyRects   : CFStringRef;

    pub(crate) static kCGDisplayStreamSourceRect          : CFStringRef;
    pub(crate) static kCGDisplayStreamDestinationRect     : CFStringRef;
    pub(crate) static kCGDisplayStreamPreserveAspectRatio : CFStringRef;
    pub(crate) static kCGDisplayStreamColorSpace          : CFStringRef;
    pub(crate) static kCGDisplayStreamMinimumFrameTime    : CFStringRef;
    pub(crate) static kCGDisplayStreamShowCursor          : CFStringRef;
    pub(crate) static kCGDisplayStreamQueueDepth          : CFStringRef;
    pub(crate) static kCGDisplayStreamYCbCrMatrix         : CFStringRef;

    static kCGDisplayStreamYCbCrMatrix_ITU_R_709_2     : CFStringRef;
    static kCGDisplayStreamYCbCrMatrix_ITU_R_601_4     : CFStringRef;
    static kCGDisplayStreamYCbCrMatrix_SMPTE_240M_1995 : CFStringRef;

}

const SCSTREAM_ERROR_CODE_USER_STOPPED: isize = -3817;

pub const kAudioFormatFlagIsFloat          : u32 = 1 << 0;
pub const kAudioFormatFlagIsBigEndian     : u32 = 1 << 1;
pub const kAudioFormatFlagIsPacked         : u32 = 1 << 3;
pub const kAudioFormatFlagIsNonInterleaved : u32 = 1 << 5;
#[cfg(target_endian = "big")]
pub const kAudioFormatNativeEndian         : u32 = kAudioFormatFlagIsBigEndian;
#[cfg(target_endian = "little")]
pub const kAudioFormatNativeEndian         : u32 = 0;

pub const kAudioFormatFlagsCanonical       : u32 = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked | kAudioFormatNativeEndian;

pub const kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment: u32 = 1 << 0;

pub const kCMSampleBufferError_ArrayTooSmall: i32 = -12737;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub(crate) struct CGColorRef(CFTypeRef);

unsafe impl Encode for CGColorRef {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("^{CGColor=}") }
    }
}

#[repr(C)]
pub(crate) struct NSString(pub(crate) *mut Object);

impl NSString {
    pub(crate) fn from_ref_unretained(r: CFStringRef) -> Self {
        unsafe { CFRetain(r); }
        Self(r as *mut Object)
    }

    pub(crate) fn from_ref_retained(r: CFStringRef) -> Self {
        Self(r as *mut Object)
    }

    pub(crate) fn from_id_unretained(id: *mut Object) -> Self {
        unsafe {
            let _: () = msg_send![id, retain];
            Self(id)
        }
    }

    pub(crate) fn as_string(&self) -> String {
        unsafe {
            let c_str: *const i8 = msg_send![self.0, UTF8String];
            let len = strlen(c_str);
            let bytes = std::slice::from_raw_parts(c_str as *const u8, len);
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

unsafe impl Encode for NSString {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("^@\"NSString\"") }
    }
}

#[repr(C)]
pub(crate) struct NSError(*mut Object);
unsafe impl Send for NSError {}

impl NSError {
    pub(crate) fn from_id_unretained(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    pub(crate) fn from_id_retained(id: *mut Object) -> Self {
        Self(id)
    }

    pub(crate) fn code(&self) -> isize {
        unsafe { msg_send![self.0, code] }
    }

    pub(crate) fn domain(&self) -> String {
        unsafe {
            let domain_cfstringref: CFStringRef = msg_send![self.0, domain];
            NSString::from_ref_retained(domain_cfstringref).as_string()
        }
    }

    pub fn description(&self) -> String {
        unsafe { NSString::from_id_unretained(msg_send![self.0, localizedDescription]).as_string() }
    }

    pub fn reason(&self) -> String {
        unsafe { NSString::from_id_unretained(msg_send![self.0, localizedFailureReason]).as_string() }
    }

    //pub(crate) fn user_info(&self) -> 
}

unsafe impl Encode for NSError {
    fn encode() -> Encoding {
        unsafe {
            Encoding::from_str("@\"NSError\"")
        }
    }
}

impl Drop for NSError {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; };
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct NSArrayRef(*mut Object);

impl NSArrayRef {
    pub(crate) fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

unsafe impl Encode for NSArrayRef {
    fn encode() -> objc::Encoding {
        unsafe { Encoding::from_str("@\"NSArray\"") }
    }
}

#[repr(C)]
pub(crate) struct NSArray(*mut Object);

impl NSArray {
    pub(crate) fn new() -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(NSArray), new];
            Self::from_id_retained(id)
        }
    }

    pub(crate) fn new_mutable() -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(NSMutableArray), alloc];
            let id: *mut Object = msg_send![id, init];
            Self::from_id_retained(id)
        }
    }

    pub(crate) fn from_ref(r: NSArrayRef) -> Self {
        Self::from_id_unretained(r.0)
    }

    pub(crate) fn from_id_unretained(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    pub(crate) fn from_id_retained(id: *mut Object) -> Self {
        Self(id)
    }

    pub(crate) fn count(&self) -> usize {
        unsafe { msg_send![self.0, count] }
    }

    pub(crate) fn add_object<T: 'static>(&mut self, object: T) {
        unsafe {
            let _: () = msg_send![self.0, addObject: object];
        }
    }

    pub(crate) fn obj_at_index<T: 'static>(&self, i: usize) -> T {
        unsafe { msg_send![self.0, objectAtIndex: i] }
    }
}

impl Drop for NSArray {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}


#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct NSDictionaryEncoded(*mut Object);

impl NSDictionaryEncoded {
    pub(crate) fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

unsafe impl Encode for NSDictionaryEncoded {
    fn encode() -> objc::Encoding {
        unsafe { Encoding::from_str("@\"NSDictionary\"") }
    }
}


#[repr(C)]
pub(crate) struct NSDictionary(pub(crate) *mut Object);

impl NSDictionary {
    pub(crate) fn new() -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(NSDictionary), new];
            Self::from_id_retained(id)
        }
    }

    pub(crate) fn new_mutable() -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(NSMutableDictionary), new];
            Self::from_id_retained(id)
        }
    }

    pub(crate) fn from_ref_unretained(r: CGDictionaryRef) -> Self {
        Self::from_id_unretained(r as *mut Object)
    }

    pub(crate) fn from_encoded(e: NSDictionaryEncoded) -> Self {
        Self::from_id_unretained(e.0)
    }

    pub(crate) fn from_id_unretained(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    pub(crate) fn from_id_retained(id: *mut Object) -> Self {
        Self(id)
    }

    pub(crate) fn count(&self) -> usize {
        unsafe { msg_send![self.0, count] }
    }

    pub(crate) fn all_keys(&self) -> NSArray {
        unsafe {
            let keys: *mut Object = msg_send![self.0, allKeys];
            NSArray::from_id_retained(keys)
        }
    }

    pub(crate) fn value_for_key(&self, key: CFStringRef) -> *mut Object {
        unsafe {
            msg_send![self.0, valueForKey: key]
        }
    }

    pub(crate) fn set_object_for_key(&mut self, object: *mut Object, key: *mut Object) {
        unsafe {
            let _: () = msg_send![self.0, setObject: object forKey: key];
        }
    }
}

impl Drop for NSDictionary {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}

impl Clone for NSDictionary {
    fn clone(&self) -> Self {
        Self::from_id_unretained(self.0)
    }
}

type CGFloat = f64;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct CGPoint {
    pub(crate) x: CGFloat,
    pub(crate) y: CGFloat,
}

impl CGPoint {
    pub(crate) const ZERO: CGPoint = CGPoint { x: 0.0, y: 0.0 };
    pub(crate) const INF: CGPoint = CGPoint { x: std::f64::INFINITY, y: std::f64::INFINITY };
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct CGSize {
    pub(crate) x: CGFloat,
    pub(crate) y: CGFloat,
}

impl CGSize {
    pub(crate) const ZERO: CGSize = CGSize { x: 0.0, y: 0.0 };
}

unsafe impl Encode for CGSize {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGSize=\"width\"d\"height\"d}") }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct CGRect {
    pub(crate) origin: CGPoint,
    pub(crate) size: CGSize,
}

impl CGRect {
    pub(crate) const ZERO: CGRect = CGRect {
        origin: CGPoint::ZERO,
        size: CGSize::ZERO
    };

    pub(crate) const NULL: CGRect = CGRect {
        origin: CGPoint::INF,
        size: CGSize::ZERO
    };

    pub(crate) fn contains(&self, p: CGPoint) -> bool {
        p.x >= self.origin.x &&
        p.y >= self.origin.y &&
        p.x <= (self.origin.x + self.size.x) &&
        p.y <= (self.origin.y + self.size.y)
    }

    pub(crate) fn create_dicitonary_representation(&self) -> NSDictionary {
        unsafe {
            NSDictionary::from_ref_unretained(CGRectCreateDictionaryRepresentation(*self))
        }
    }
}

unsafe impl Encode for CGRect {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGRect=\"origin\"{CGPoint=\"x\"d\"y\"d}\"size\"{CGSize=\"width\"d\"height\"d}}") }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct CGWindowID(pub(crate) u32);

impl CGWindowID {
    pub(crate) fn raw(&self) -> u32 {
        self.0
    }
}

unsafe impl Send for CGWindowID {}

#[repr(C)]
pub(crate) struct SCWindow(*mut Object);
unsafe impl Send for SCWindow {}

impl SCWindow {
    pub(crate) fn from_id_unretained(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    pub(crate) fn from_id_retained(id: *mut Object) -> Self {
        Self(id)
    }

    pub(crate) fn id(&self) -> CGWindowID {
        unsafe { 
            let id: u32 = msg_send![self.0, windowID];
            CGWindowID(id)
        }
    }

    pub(crate) fn title(&self) -> String {
        unsafe {
            let title_cfstringref: CFStringRef = msg_send![self.0, title];
            NSString::from_ref_unretained(title_cfstringref).as_string()
        }
    }

    pub(crate) fn frame(&self) -> CGRect {
        unsafe {
            *(*self.0).get_ivar("_frame")
        }
    }

    pub(crate) fn owning_application(&self) -> SCRunningApplication {
        unsafe {
            let scra_id: *mut Object = msg_send![self.0, owningApplication];
            SCRunningApplication::from_id_unretained(scra_id)
        }
    }
}

impl Clone for SCWindow {
    fn clone(&self) -> Self {
        unsafe { let _: () = msg_send![self.0, retain]; }
        SCWindow(self.0)
    }
}

impl Drop for SCWindow {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}

#[repr(C)]
pub(crate) struct SCDisplay(*mut Object);
unsafe impl Send for SCDisplay {}

impl SCDisplay {
    pub(crate) fn from_id_unretained(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    pub(crate) fn from_id_retained(id: *mut Object) -> Self {
        Self(id)
    }

    pub(crate) fn frame(&self) -> CGRect {
        unsafe {
            *(*self.0).get_ivar("_frame")
        }
    }

    pub(crate) fn raw_id(&self) -> u32 {
        unsafe {
            *(*self.0).get_ivar("_displayID")
        }
    }
}

impl Clone for SCDisplay {
    fn clone(&self) -> Self {
        unsafe { let _: () = msg_send![self.0, retain]; }
        SCDisplay(self.0)
    }
}

impl Drop for SCDisplay {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}

#[repr(C)]
pub(crate) struct SCShareableContent(*mut Object);
unsafe impl Send for SCShareableContent {}
unsafe impl Sync for SCShareableContent {}

impl SCShareableContent {
    pub(crate) fn get_shareable_content_with_completion_handler(
        excluding_desktop_windows: bool,
        onscreen_windows_only: bool,
        completion_handler: impl Fn(Result<SCShareableContent, NSError>) + Send + 'static,
    ) {
        let completion_handler = Box::new(completion_handler);
        let handler_block = ConcreteBlock::new(move |sc_shareable_content: *mut Object, error: *mut Object| {
            if !error.is_null() {
                let error = NSError::from_id_retained(error);
                (completion_handler)(Err(error));
            } else {
                unsafe { let _:() = msg_send![sc_shareable_content, retain]; }
                let sc_shareable_content = SCShareableContent(sc_shareable_content);
                (completion_handler)(Ok(sc_shareable_content));
            }
        }).copy();
        unsafe {
            let _: () = msg_send![
                class!(SCShareableContent),
                getShareableContentExcludingDesktopWindows: Bool::from_raw(excluding_desktop_windows)
                onScreenWindowsOnly: Bool::from_raw(onscreen_windows_only)
                completionHandler: handler_block
            ];
        }
    }

    pub(crate) fn windows(&self) -> Vec<SCWindow> {
        let mut windows = Vec::new();
        unsafe {
            let windows_nsarray_ref: NSArrayRef = *(*self.0).get_ivar("_windows");
            if !windows_nsarray_ref.is_null() {
                let windows_ns_array = NSArray::from_ref(windows_nsarray_ref);
                let count = windows_ns_array.count();
                for i in 0..count {
                    let window_id: *mut Object = windows_ns_array.obj_at_index(i);
                    windows.push(SCWindow::from_id_unretained(window_id));
                }
            }
        }
        windows
    }

    pub(crate) fn displays(&self) -> Vec<SCDisplay> {
        let mut displays = Vec::new();
        unsafe {
            let displays_ref: NSArrayRef = *(*self.0).get_ivar("_displays");
            if !displays_ref.is_null() {
                let displays_ns_array = NSArray::from_ref(displays_ref);
                let count = displays_ns_array.count();
                for i in 0..count {
                    let display_id: *mut Object = displays_ns_array.obj_at_index(i);
                    displays.push(SCDisplay::from_id_unretained(display_id));
                }
            }
        }
        displays
    }

    pub(crate) fn applications(&self) -> Vec<SCRunningApplication> {
        let mut applications = Vec::new();
        unsafe {
            let applicaitons_ref: NSArrayRef = *(*self.0).get_ivar("_applications");
            if !applicaitons_ref.is_null() {
                let applications_array = NSArray::from_ref(applicaitons_ref);
                let count = applications_array.count();
                for i in 0..count {
                    let application_id: *mut Object = applications_array.obj_at_index(i);
                    applications.push(SCRunningApplication::from_id_unretained(application_id));
                }
            }
        }
        applications
    }
}

impl Drop for SCShareableContent {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub(crate) struct OSType([u8; 4]);

impl OSType {
    pub fn as_i32(&self) -> i32 {
        unsafe { std::mem::transmute(self.0) }
    }

    pub fn as_u32(&self) -> u32 {
        unsafe { std::mem::transmute(self.0) }
    }
}

unsafe impl Encode for OSType {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("I") }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub(crate) enum SCStreamPixelFormat {
    BGRA8888,
    L10R,
    V420,
    F420,
}

impl SCStreamPixelFormat {
    pub(crate) fn to_ostype(&self) -> OSType {
        match self {
            Self::BGRA8888 => OSType(['B' as u8, 'G' as u8, 'R' as u8, 'A' as u8]),
            Self::L10R     => OSType(['l' as u8, '1' as u8, '0' as u8, 'r' as u8]),
            Self::V420     => OSType(['4' as u8, '2' as u8, '0' as u8, 'v' as u8]),
            Self::F420     => OSType(['4' as u8, '2' as u8, '0' as u8, 'f' as u8]),
        }
    }
}


#[derive(Copy, Clone, Debug)]
pub(crate) enum SCStreamBackgroundColor {
    Black,
    White,
    Clear,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SCStreamColorMatrix {
    ItuR709_2,
    ItuR601_4,
    Smpte240M1995,
}

impl SCStreamColorMatrix {
    pub(crate) fn to_cfstringref(&self) -> CFStringRef {
        unsafe {
            match self {
                Self::ItuR709_2 => kCGDisplayStreamYCbCrMatrix_ITU_R_709_2,
                Self::ItuR601_4 => kCGDisplayStreamYCbCrMatrix_ITU_R_709_2,
                Self::Smpte240M1995 => kCGDisplayStreamYCbCrMatrix_SMPTE_240M_1995,
            }
        }
    }
}

#[repr(C)]
pub(crate) struct SCStreamConfiguration(pub(crate) *mut Object);
unsafe impl Send for SCStreamConfiguration {}
unsafe impl Sync for SCStreamConfiguration {}

#[test]
fn test_sc_stream_config_properties() {
    debug_objc_class("SCStreamConfiguration");
}

impl SCStreamConfiguration {
    pub(crate) fn new() -> Self {
        unsafe {
            let instance: *mut Object = msg_send![class!(SCStreamConfiguration), alloc];
            let instance: *mut Object = msg_send![instance, init];
            Self(instance)
        }
    }

    pub(crate) fn set_size(&mut self, size: CGSize) {
        let CGSize { x, y } = size;
        unsafe {
            let _: () = msg_send![self.0, setWidth: x];
            let _: () = msg_send![self.0, setHeight: y];
        }
    }

    pub(crate) fn set_source_rect(&mut self, source_rect: CGRect) {
        unsafe {
            let _: () = msg_send![self.0, setSourceRect: source_rect];
        }
    }

    pub(crate) fn set_scales_to_fit(&mut self, scale_to_fit: bool) {
        unsafe {
            let _: () = msg_send![self.0, setScalesToFit: scale_to_fit];
        }
    }

    pub(crate) fn set_pixel_format(&mut self, format: SCStreamPixelFormat) {
        unsafe {
            let _: () = msg_send![self.0, setPixelFormat: format.to_ostype().as_u32()];
        }
    }

    pub(crate) fn set_color_matrix(&mut self, color_matrix: SCStreamColorMatrix) {
        unsafe {
            let _: () = msg_send![self.0, setColorMatrix: color_matrix.to_cfstringref()];
        }
    }

    pub(crate) fn set_background_color(&mut self, bg_color: SCStreamBackgroundColor) {
        unsafe {
            let bg_color_name = match bg_color {
                SCStreamBackgroundColor::Black => kCGColorBlack,
                SCStreamBackgroundColor::White => kCGColorWhite,
                SCStreamBackgroundColor::Clear => kCGColorClear,
            };
            let bg_color = CGColorGetConstantColor(bg_color_name);
            (*self.0).set_ivar("_backgroundColor", bg_color);
        }
    }

    pub(crate) fn set_queue_depth(&mut self, queue_depth: isize) {
        unsafe {
            let _: () = msg_send![self.0, setQueueDepth: queue_depth];
        }
    }

    pub(crate) fn set_minimum_time_interval(&mut self, interval: CMTime) {
        unsafe {
            (*self.0).set_ivar("_minimumFrameInterval", interval);
        }
    }

    pub(crate) fn set_sample_rate(&mut self, sample_rate: SCStreamSampleRate) {
        unsafe {
            (*self.0).set_ivar("_sampleRate", sample_rate.to_isize());
        }
    }

    pub(crate) fn set_show_cursor(&mut self, show_cursor: bool) {
        unsafe {
            (*self.0).set_ivar("_showsCursor", show_cursor);
        }
    }

    pub(crate) fn set_capture_audio(&mut self, capture_audio: bool) {
        unsafe {
            (*self.0).set_ivar("_capturesAudio", BOOL::from(capture_audio));
        }
    }

    pub(crate) fn set_channel_count(&mut self, channel_count: isize) {
        unsafe {
            (*self.0).set_ivar("_channelCount", channel_count);
        }
    }

    pub(crate) fn set_exclude_current_process_audio(&mut self, exclude_current_process_audio: bool) {
        unsafe {
            (*self.0).set_ivar("_excludesCurrentProcessAudio", exclude_current_process_audio);
        }
    }
}

impl Clone for SCStreamConfiguration {
    fn clone(&self) -> Self {
        unsafe { let _: () = msg_send![self.0, retain]; }
        SCStreamConfiguration(self.0)
    }
}

impl Drop for SCStreamConfiguration {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}


#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub(crate) struct CMTime {
    value: i64,
    scale: i32,
    epoch: i64,
    flags: u32,
}

pub(crate) const K_CMTIME_FLAGS_VALID                   : u32 = 1 << 0;
pub(crate) const K_CMTIME_FLAGS_HAS_BEEN_ROUNDED        : u32 = 1 << 0;
pub(crate) const K_CMTIME_FLAGS_POSITIVE_INFINITY       : u32 = 1 << 0;
pub(crate) const K_CMTIME_FLAGS_NEGATIVE_INFINITY       : u32 = 1 << 0;
pub(crate) const K_CMTIME_FLAGS_INDEFINITE              : u32 = 1 << 0;
pub(crate) const K_CMTIME_FLAGS_IMPLIED_VALUE_FLAG_MASK : u32 = 
    K_CMTIME_FLAGS_VALID                    |
    K_CMTIME_FLAGS_HAS_BEEN_ROUNDED         |
    K_CMTIME_FLAGS_POSITIVE_INFINITY        |
    K_CMTIME_FLAGS_NEGATIVE_INFINITY        |
    K_CMTIME_FLAGS_INDEFINITE
    ;

const K_CMTIME_ROUNDING_METHOD_ROUND_HALF_AWAY_FROM_ZERO: u32 = 1;
const K_CMTIME_ROUNDING_METHOD_ROUND_TOWARD_ZERO: u32 = 2;
const K_CMTIME_ROUNDING_METHOD_ROUND_AWAY_FROM_ZERO: u32 = 3;
const K_CMTIME_ROUNDING_METHOD_QUICKTIME: u32 = 4;
const K_CMTIME_ROUNDING_METHOD_TOWARD_POSITIVE_INFINITY: u32 = 5;
const K_CMTIME_ROUNDING_METHOD_TOWARD_NEGATIVE_INFINITY: u32 = 6;

#[derive(Copy, Clone, Debug)]
pub(crate) enum CMTimeRoundingMethod {
    HalfAwayFromZero,
    TowardZero,
    AwayFromZero,
    QuickTime,
    TowardPositiveInfinity,
    TowardNegativeInfinity,
}

impl CMTimeRoundingMethod {
    pub(crate) fn to_u32(&self) -> u32 {
        match self {
            Self::HalfAwayFromZero       => K_CMTIME_ROUNDING_METHOD_ROUND_HALF_AWAY_FROM_ZERO,
            Self::TowardZero             => K_CMTIME_ROUNDING_METHOD_ROUND_TOWARD_ZERO,
            Self::AwayFromZero           => K_CMTIME_ROUNDING_METHOD_ROUND_AWAY_FROM_ZERO,
            Self::QuickTime              => K_CMTIME_ROUNDING_METHOD_QUICKTIME,
            Self::TowardPositiveInfinity => K_CMTIME_ROUNDING_METHOD_TOWARD_POSITIVE_INFINITY,
            Self::TowardNegativeInfinity => K_CMTIME_ROUNDING_METHOD_TOWARD_NEGATIVE_INFINITY,
        }
    }
}

impl Default for CMTimeRoundingMethod {
    fn default() -> Self {
        Self::HalfAwayFromZero
    }
}

impl Add for CMTime {
    type Output = CMTime;

    fn add(self, rhs: Self) -> Self {
        unsafe { CMTimeAdd(self, rhs) }
    }
}

impl Sub for CMTime {
    type Output = CMTime;

    fn sub(self, rhs: Self) -> Self {
        unsafe { CMTimeSubtract(self, rhs) }
    }
}

impl Mul<i32> for CMTime {
    type Output = Self;

    fn mul(self, rhs: i32) -> Self {
        unsafe { CMTimeMultiply(self, rhs) }
    }
}

impl Mul<f64> for CMTime {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        unsafe { CMTimeMultiplyByFloat64(self, rhs) }
    }
}

impl PartialEq for CMTime {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            CMTimeCompare(*self, *other) == 0
        }
    }
}

impl PartialOrd for CMTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        unsafe {
            let self_op_other = CMTimeCompare(*self, *other);
            match self_op_other {
                -1 => Some(std::cmp::Ordering::Less),
                0 => Some(std::cmp::Ordering::Equal),
                1 => Some(std::cmp::Ordering::Greater),
                _ => None
            }
        }
    }
}

unsafe impl Encode for CMTime {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{?=\"value\"q\"timescale\"i\"flags\"I\"epoch\"q}") }
    }
}

impl CMTime {
    pub(crate) fn new_with_seconds(seconds: f64, timescale: i32) -> Self {
        unsafe { CMTimeMakeWithSeconds(seconds, timescale) }
    }

    pub(crate) const fn is_valid(&self) -> bool {
        self.flags & K_CMTIME_FLAGS_VALID != 0
    }

    pub(crate) const fn is_invalid(&self) -> bool {
        ! self.is_valid()
    }

    pub(crate) const fn is_indefinite(&self) -> bool {
        self.is_valid() &&
        (self.flags & K_CMTIME_FLAGS_INDEFINITE != 0)
    }

    pub(crate) const fn is_positive_infinity(&self) -> bool {
        self.is_valid() &&
        (self.flags & K_CMTIME_FLAGS_POSITIVE_INFINITY != 0)
    }

    pub(crate) const fn is_negative_infinity(&self) -> bool {
        self.is_valid() &&
        (self.flags & K_CMTIME_FLAGS_NEGATIVE_INFINITY != 0)
    }

    pub(crate) const fn is_numeric(&self) -> bool {
        self.is_valid() &&
        ! self.is_indefinite() &&
        ! self.is_positive_infinity() &&
        ! self.is_negative_infinity()
    }

    pub(crate) const fn has_been_rounded(&self) -> bool {
        self.flags & K_CMTIME_FLAGS_HAS_BEEN_ROUNDED != 0
    }

    pub(crate) fn convert_timescale(&self, new_timescale: i32, rounding_method: CMTimeRoundingMethod) -> Self {
        unsafe { CMTimeConvertScale(*self, new_timescale, rounding_method.to_u32()) }
    }

    pub(crate) fn multiply_by_ratio(&self, multiplier: i32, divisor: i32) -> Self {
        unsafe { CMTimeMultiplyByRatio(*self, multiplier, divisor) }
    }

    pub(crate) fn seconds_f64(&self) -> f64 {
        unsafe { CMTimeGetSeconds(*self) }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum SCStreamSampleRate {
    R8000,
    R16000,
    R24000,
    R48000,
}

impl SCStreamSampleRate {
    pub(crate) fn to_isize(&self) -> isize {
        match self {
            Self::R8000  => 8000,
            Self::R16000 => 16000,
            Self::R24000 => 24000,
            Self::R48000 => 48000,
        }
    }
}

#[repr(C)]
pub(crate) struct SCContentFilter(pub(crate) *mut Object);

unsafe impl Send for SCContentFilter {}
unsafe impl Sync for SCContentFilter {}

impl SCContentFilter {
    pub(crate) fn new_with_desktop_independent_window(window: SCWindow) -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(SCContentFilter), alloc];
            let _: *mut Object = msg_send![id, initWithDesktopIndependentWindow: window.0];
            Self(id)
        }
    }

    pub(crate) fn new_with_display_excluding_apps_excepting_windows(display: SCDisplay, excluded_applications: NSArray, excepting_windows: NSArray) -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(SCContentFilter), alloc];
            let id: *mut Object = msg_send![id, initWithDisplay: display.0 excludingApplications: excluded_applications.0 exceptingWindows: excepting_windows.0];
            Self(id)
        }
    }
}

impl Clone for SCContentFilter {
    fn clone(&self) -> Self {
        unsafe { let _: () = msg_send![self.0, retain]; }
        SCContentFilter(self.0)
    }
}

impl Drop for SCContentFilter {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}

pub(crate) enum SCStreamCallbackError {
    SampleBufferCopyFailed,
    StreamStopped,
    Other(NSError)
}

#[repr(C)]
struct SCStreamCallbackContainer {
    callback: Box<dyn FnMut(Result<(CMSampleBuffer, SCStreamOutputType), SCStreamCallbackError>) + Send + 'static>
}

impl SCStreamCallbackContainer {
    pub fn new(callback: impl FnMut(Result<(CMSampleBuffer, SCStreamOutputType), SCStreamCallbackError>) + Send + 'static) -> Self {
        Self {
            callback: Box::new(callback)
        }
    }

    pub fn call_output(&mut self, sample_buffer: CMSampleBuffer, output_type: SCStreamOutputType) {
        (self.callback)(Ok((sample_buffer, output_type)));
    }

    pub fn call_error(&mut self, error: SCStreamCallbackError) {
        (self.callback)(Err(error));
    }
}

#[derive(Copy, Clone, Debug)]
pub enum SCStreamOutputType {
    Screen,
    Audio,
}

impl SCStreamOutputType {
    pub fn to_encoded(&self) -> SCStreamOutputTypeEncoded {
        SCStreamOutputTypeEncoded(match *self {
            Self::Screen => 0,
            Self::Audio => 1,
        })
    }
}

#[repr(C)]
struct SCStreamOutputTypeEncoded(usize);

unsafe impl Encode for SCStreamOutputTypeEncoded {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("q") }
    }
}

#[repr(C)]
struct SCStreamEncoded(*mut Object);

unsafe impl Message for SCStreamEncoded {}

unsafe impl Encode for SCStreamEncoded {
    fn encode() -> Encoding {
        unsafe {
            Encoding::from_str("@\"SCStream\"")
        }
    }
}

extern fn sc_stream_output_did_output_sample_buffer_of_type(this: &Object, _sel: Sel, stream: SCStream, buffer: CMSampleBufferRef, output_type: SCStreamOutputTypeEncoded) {
    unsafe {
        println!("sc_stream_output_did_output_sample_buffer_of_type(this: {:?}, stream: {:?}, buffer: {:?}, output_type: {:?})", this, stream.0, buffer, output_type.0);
        std::mem::forget(stream);
    }
}

extern fn sc_stream_handler_did_stop_with_error(this: &Object, _sel: Sel, stream: SCStream, error: NSError) -> () {
    unsafe {
        println!("sc_stream_handler_did_stop_with_error(this: {:?}, stream: {:?}, error: {:?})", this, stream.0, error.0);
        std::mem::forget(error);
        std::mem::forget(stream);
    }
}

extern fn sc_stream_handler_dealloc(this: &mut Object, _sel: Sel) {
    unsafe {
        println!("sc_stream_handler_dealloc(this: {:?})", this);
    }
}

#[repr(C)]
pub(crate) struct SCStreamHandler(*mut Object);

unsafe impl Message for SCStreamHandler {}

unsafe impl Encode for SCStreamHandler {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("@\"<SCStreamOutput, SCStreamDelegate>\"") }
    }
}

impl SCStreamHandler {
    pub fn new(callback: impl FnMut(Result<(CMSampleBuffer, SCStreamOutputType), SCStreamCallbackError>) + Send + 'static) -> Self {
        let class = Self::get_class();
        let callback_container_ptr = Box::leak(Box::new(SCStreamCallbackContainer::new(callback)));
        unsafe {
            let instance: *mut Object = msg_send![class!(SCStreamHandler), alloc];
            let instance: *mut Object = msg_send![instance, init];
            (*instance).set_ivar("callback_container_ptr", callback_container_ptr as *mut _ as *mut c_void);
            Self(instance)
        }
    }

    fn get_class() -> &'static Class {
        unsafe {
            let class_name = CString::new("SCStreamHandler").unwrap();
            let class_ptr = objc::runtime::objc_getClass(class_name.as_ptr());
            if !class_ptr.is_null() {
                &*class_ptr
            } else if let Some(mut class) = objc::declare::ClassDecl::new("SCStreamHandler", class!(NSObject)) {
                class.add_method(sel!(stream:didOutputSampleBuffer:ofType:), sc_stream_output_did_output_sample_buffer_of_type as extern fn (&Object, Sel, SCStream, CMSampleBufferRef, SCStreamOutputTypeEncoded));
                class.add_method(sel!(stream:didStopWithError:), sc_stream_handler_did_stop_with_error as extern fn(&Object, Sel, SCStream, NSError));
                class.add_method(sel!(dealloc), sc_stream_handler_dealloc as extern fn(&mut Object, Sel));
                
                //let sc_stream_delegate_name = CString::new("SCStreamDelegate").unwrap();
                //class.add_protocol(&*objc::runtime::objc_getProtocol(sc_stream_delegate_name.as_ptr()));
                
                //let sc_stream_output_name = CString::new("SCStreamOutput").unwrap();
                //class.add_protocol(&*objc::runtime::objc_getProtocol(sc_stream_output_name.as_ptr()));

                class.add_ivar::<*mut c_void>("callback_container_ptr");
                
                class.register()
            } else {
                panic!("Failed to register SCStreamHandler");
            }
        }
    }
}


#[repr(C)]
pub struct SCStream(*mut Object);

unsafe impl Sync for SCStream {}
unsafe impl Send for SCStream {}

unsafe impl Encode for SCStream {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("@\"SCStream\"") }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct SCStreamDelegate(*mut Object);

unsafe impl Encode for SCStreamDelegate {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("@\"<SCStreamDelegate>\"") }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct SCStreamOutput(*mut Object);

unsafe impl Encode for SCStreamOutput {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("@\"<SCStreamOutput>\"") }
    }
}

impl SCStream {
    pub fn preflight_access() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    pub async fn request_access() -> bool {
        async {
            unsafe { CGRequestScreenCaptureAccess() }
        }.await
    }

    pub fn from_id(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    pub fn is_nil(&self) -> bool {
        self.0.is_null()
    }

    pub fn new(filter: SCContentFilter, config: SCStreamConfiguration, handler_queue: DispatchQueue, handler: SCStreamHandler) -> Result<Self, String> {
        unsafe {
            let instance: *mut Object = msg_send![class!(SCStream), alloc];
            let instance: *mut Object = msg_send![instance, initWithFilter: filter.0 configuration: config.0 delegate: SCStreamDelegate(handler.0)];
            println!("SCStream instance: {:?}", instance);
            let mut error: *mut Object = std::ptr::null_mut();
            let result: bool = msg_send![instance, addStreamOutput: SCStreamOutput(handler.0) type: SCStreamOutputType::Screen.to_encoded() sampleHandlerQueue: handler_queue error: &mut error as *mut _];
            println!("addStreamOutput result: {}", result);
            if !error.is_null() {
                let error = NSError::from_id_retained(error);
                println!("error: {}, reason: {}", error.description(), error.reason());
            }
            Ok(SCStream(instance))
        }
    }

    pub fn start(&mut self) {
        unsafe {
            let _: () = msg_send![self.0, startCaptureWithCompletionHandler: ConcreteBlock::new(Box::new(|error: *mut Object| {
                if !error.is_null() {
                    let error =  NSError::from_id_unretained(error);
                    println!("startCaptureWithCompletionHandler error: {:?}, reason: {:?}", error.description(), error.reason());
                } else {
                    println!("startCaptureWithCompletionHandler success!");
                }
            })).copy()];
        }
    }

    pub fn stop(&mut self) {
    }
}

#[repr(C)]
pub(crate) struct CMSampleBuffer(CMSampleBufferRef);

impl CMSampleBuffer {
    pub(crate) fn copy_from_ref(r: CMSampleBufferRef) -> Result<Self, ()> {
        unsafe { 
            let mut new_ref: CMSampleBufferRef = std::ptr::null();
            let status = CMSampleBufferCreateCopy(kCFAllocatorNull, r, &mut new_ref as *mut _);
            if status != 0 {
                Err(())
            } else {
                Ok(CMSampleBuffer(new_ref))
            }
        }
    }

    pub(crate) fn get_presentation_timestamp(&self) -> CMTime {
        unsafe { CMSampleBufferGetPresentationTimeStamp(self.0) }
    }

    pub(crate) fn get_duration(&self) -> CMTime {
        unsafe { CMSampleBufferGetDuration(self.0) }
    }

    pub(crate) fn get_format_description(&self) -> CMFormatDescription {
        let format_desc_ref = unsafe { CMSampleBufferGetFormatDescription(self.0) };
        CMFormatDescription::from_ref_unretained(format_desc_ref)
    }

    // CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer
    pub(crate) unsafe fn get_audio_buffer_list_with_block_buffer(&self) -> Result<(AudioBufferList, CMBlockBuffer), ()> {
        let mut audio_buffer_list = AudioBufferList::default();
        let mut block_buffer: CMBlockBufferRef = std::ptr::null();
        let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            self.0,
            std::ptr::null_mut(),
            &mut audio_buffer_list as *mut _,
            std::mem::size_of::<AudioBufferList>(),
            kCFAllocatorNull,
            kCFAllocatorNull,
            kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
            &mut block_buffer as *mut _
        );
        if status != 0 {
            println!("CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(...) failed: {}", status);
            return Err(());
        }
        Ok((audio_buffer_list, CMBlockBuffer::from_ref_retained(block_buffer)))
    }

    pub(crate) fn get_sample_attachment_array(&self) -> Vec<CFDictionary> {
        unsafe {
            let attachment_array_ref = CMSampleBufferGetSampleAttachmentsArray(self.0, false.into());
            if attachment_array_ref.is_null() {
                return vec![];
            }
            let attachments_array = CFArray::from_ref_unretained(attachment_array_ref);
            let mut attachments = Vec::new();
            for i in 0..attachments_array.get_count() {
                attachments.push(CFDictionary::from_ref_unretained(attachments_array.get_value_at_index(i)));
            }
            attachments
        }
    }
}

impl Clone for CMSampleBuffer {
    fn clone(&self) -> Self {
        unsafe { CFRetain(self.0); }
        Self(self.0)
    }
}

impl Drop for CMSampleBuffer {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0); }
    }
}

#[repr(C)]
pub(crate) struct CFDictionary(CFTypeRef);

impl CFDictionary {
    pub(crate) fn from_ref_retained(r: CFDictionaryRef) -> Self {
        Self(r)
    }

    pub(crate) fn from_ref_unretained(r: CFDictionaryRef) -> Self {
        unsafe { CFRetain(r); }
        Self(r)
    }

    pub(crate) fn get_value(&self, key: CFTypeRef) -> CFTypeRef {
        unsafe { CFDictionaryGetValue(self.0, key) }
    }
}

impl Clone for CFDictionary {
    fn clone(&self) -> Self {
        Self::from_ref_unretained(self.0)
    }
}

impl Drop for CFDictionary {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0); }
    }
}

pub(crate) enum CMMediaType {
    Audio,
    Video,
}

impl CMMediaType {
    pub(crate) fn from_ostype(ostype: OSType) -> Option<Self> {
        match ostype.0.map(|x| x as char) {
            ['v', 'i', 'd', 'e'] => Some(Self::Video),
            ['s', 'o', 'u', 'n'] => Some(Self::Audio),
            _ => None,
        }
    }
}

#[repr(C)]
pub(crate) struct CMFormatDescription(CMFormatDescriptionRef);

impl CMFormatDescription {
    pub(crate) fn from_ref_retained(r: CMFormatDescriptionRef) -> Self {
        Self(r)
    }

    pub(crate) fn from_ref_unretained(r: CMFormatDescriptionRef) -> Self {
        unsafe { CFRetain(r); }
        Self(r)
    }

    pub(crate) fn get_media_type(&self) -> OSType {
        unsafe { CMFormatDescriptionGetMediaType(self.0) }
    }

    pub(crate) fn as_audio_format_description(&self) -> Option<CMAudioFormatDescription> {
        match CMMediaType::from_ostype(self.get_media_type()) {
            Some(CMMediaType::Audio) => {
                Some(CMAudioFormatDescription::from_ref_unretained(self.0))
            },
            _ => None
        }
    }
}

impl Drop for CMFormatDescription {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0); }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub(crate) struct AudioStreamBasicDescription {
    pub sample_rate: f64,
    pub format_id: u32,
    pub format_flags: u32,
    pub bytes_per_packet: u32,
    pub frames_per_packet: u32,
    pub bytes_per_frame: u32,
    pub channels_per_frame: u32,
    pub bits_per_channel: u32,
    pub reserved: u32,
}

#[repr(C)]
pub(crate) struct CMAudioFormatDescription(CMFormatDescriptionRef);

impl CMAudioFormatDescription {
    pub(crate) fn from_ref_retained(r: CMFormatDescriptionRef) -> Self {
        Self(r)
    }

    pub(crate) fn from_ref_unretained(r: CMFormatDescriptionRef) -> Self {
        unsafe { CFRetain(r); }
        Self(r)
    }

    pub(crate) fn get_basic_stream_description(&self) -> &'_ AudioStreamBasicDescription {
        unsafe { &*CMAudioFormatDescriptionGetStreamBasicDescription(self.0) as &_ }
    }
}

impl Drop for CMAudioFormatDescription {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0); }
    }
}

pub(crate) struct AVAudioFormat(*mut Object);

impl AVAudioFormat {
    pub fn new_with_standard_format_sample_rate_channels(sample_rate: f64, channel_count: u32) -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(AVAudioFormat), alloc];
            let _: *mut Object = msg_send![id, initStandardFormatWithSampleRate: sample_rate channels: channel_count];
            Self(id)
        }
    }
}

impl Drop for AVAudioFormat {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}

#[repr(C)]
pub(crate) struct AudioBuffer {
    number_channels: u32,
    data_byte_size: u32,
    data: *mut c_void,
}

#[repr(C)]
pub(crate) struct AudioBufferList {
    number_buffers: u32,
    buffers: *mut AudioBuffer,
}

impl Default for AudioBufferList {
    fn default() -> Self {
        Self {
            number_buffers: 0,
            buffers: std::ptr::null_mut()
        }
    }
}

#[repr(C)]
pub(crate) struct CMBlockBuffer(CMBlockBufferRef);


impl CMBlockBuffer {
    pub(crate) fn from_ref_retained(r: CMBlockBufferRef) -> Self {
        Self(r)
    }

    pub(crate) fn from_ref_unretained(r: CMBlockBufferRef) -> Self {
        unsafe { CFRetain(r); }
        Self(r)
    }
}

impl Drop for CMBlockBuffer {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0); }
    }
}

pub(crate) struct AVAudioPCMBuffer(*mut Object);

impl AVAudioPCMBuffer {
    pub fn new_with_format_buffer_list_no_copy_deallocator(format: AVAudioFormat, buffer_list_no_copy: *const AudioBufferList) -> Result<Self, ()> {
        unsafe {
            let id: *mut Object = msg_send![class!(AVAudioPCMBuffer), alloc];
            let result: *mut Object = msg_send![id, initWithPCMFormat: format bufferListNoCopy: buffer_list_no_copy];
            if result.is_null() {
                let _: () = msg_send![id, release];
                Err(())
            } else {
                Ok(Self(id))
            }
        }
    }

    pub fn stride(&self) -> usize {
        unsafe { msg_send![self.0, stride] }
    }

    pub fn channel_count(&self) -> usize {
        unsafe { msg_send![self.0, stride] }
    }

    pub fn f32_buffer(&self, channel: usize) -> Option<*const f32> {
        let channel_count = self.stride();
        if channel >= channel_count {
            return None;
        }
        unsafe {
            let all_channels_data_ptr: *const *const f32 = msg_send![self.0, floatChannelData];
            if all_channels_data_ptr.is_null() {
                return None;
            }
            let all_channels_data = std::slice::from_raw_parts(all_channels_data_ptr, channel_count);
            let channel_data = all_channels_data[channel];
            if channel_data.is_null() {
                None
            } else {
                Some(channel_data)
            }
        }
    }

    pub fn i32_buffer(&self, channel: usize) -> Option<*const i32> {
        let channel_count = self.stride();
        if channel >= channel_count {
            return None;
        }
        unsafe {
            let all_channels_data_ptr: *const *const i32 = msg_send![self.0, int32ChannelData];
            if all_channels_data_ptr.is_null() {
                return None;
            }
            let all_channels_data = std::slice::from_raw_parts(all_channels_data_ptr, channel_count);
            let channel_data = all_channels_data[channel];
            if channel_data.is_null() {
                None
            } else {
                Some(channel_data)
            }
        }
    }

    pub fn i16_buffer(&self, channel: usize) -> Option<*const i16> {
        let channel_count = self.stride();
        if channel >= channel_count {
            return None;
        }
        unsafe {
            let all_channels_data_ptr: *const *const i16 = msg_send![self.0, int32ChannelData];
            if all_channels_data_ptr.is_null() {
                return None;
            }
            let all_channels_data = std::slice::from_raw_parts(all_channels_data_ptr, channel_count);
            let channel_data = all_channels_data[channel];
            if channel_data.is_null() {
                None
            } else {
                Some(channel_data)
            }
        }
    }
}

#[repr(C)]
struct CFArray(CFArrayRef);

impl CFArray {
    pub(crate) fn from_ref_unretained(r: CFStringRef) -> Self {
        unsafe { CFRetain(r); }
        Self(r)
    }

    pub(crate) fn from_ref_retained(r: CFStringRef) -> Self {
        Self(r)
    }

    pub(crate) fn get_count(&self) -> i32 {
        unsafe { CFArrayGetCount(self.0) }
    }

    pub(crate) fn get_value_at_index(&self, index: i32) -> *const c_void {
        unsafe { CFArrayGetValueAtIndex(self.0, index) }
    }
}

impl Clone for CFArray {
    fn clone(&self) -> Self {
        CFArray::from_ref_unretained(self.0)
    }
}

impl Drop for CFArray {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0); }
    }
}




#[repr(C)]
#[derive(Debug)]
pub struct DispatchQueue(*mut Object);

impl DispatchQueue {
    pub fn make_concurrent(name: String) -> Self {
        let cstring_name = CString::new(name.as_str()).unwrap();
        unsafe { dispatch_queue_create(cstring_name.as_ptr(), DispatchQueueAttr(&mut _dispatch_queue_attr_concurrent as *mut c_void)) }
    }

    pub fn make_serial(name: String) -> Self {
        let cstring_name = CString::new(name.as_str()).unwrap();
        unsafe { dispatch_queue_create(cstring_name.as_ptr(), DispatchQueueAttr(0 as *mut c_void)) }
    }

    pub fn make_null() -> Self {
        DispatchQueue(std::ptr::null_mut())
    }
}

impl Drop for DispatchQueue {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        unsafe { dispatch_release(self.0) };
    }
}

impl Clone for DispatchQueue {
    fn clone(&self) -> Self {
        if self.0.is_null() {
            return Self(std::ptr::null_mut());
        }
        unsafe { dispatch_retain(self.0); }
        Self(self.0)
    }
}

unsafe impl Encode for DispatchQueue {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("@\"NSObject<OS_dispatch_queue>\"") }
    }
}

#[repr(C)]
struct DispatchQueueAttr(*mut c_void);

pub(crate) struct SCRunningApplication(pub(crate) *mut Object);

impl SCRunningApplication {
    pub(crate) fn from_id_unretained(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    pub(crate) fn from_id_retained(id: *mut Object) -> Self {
        Self(id)
    }

    pub(crate) fn pid(&self) -> i32 {
        unsafe {
            msg_send![self.0, processID]
        }
    }

    pub(crate) fn application_name(&self) -> String {
        unsafe {
            let app_name_cfstringref: CFStringRef = msg_send![self.0, applicationName];
            NSString::from_ref_unretained(app_name_cfstringref).as_string()
        }
    }

    pub(crate) fn bundle_identifier(&self) -> String {
        unsafe {
            let bundle_id_cfstringref: CFStringRef = msg_send![self.0, bundleIdentifier];
            NSString::from_ref_unretained(bundle_id_cfstringref).as_string()
        }
    }
}

impl Clone for SCRunningApplication {
    fn clone(&self) -> Self {
        Self::from_id_unretained(self.0)
    }
}

impl Drop for SCRunningApplication {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum CGDisplayStreamFrameStatus {
    Complete,
    Idle,
    Blank,
    Stopped,
}

impl CGDisplayStreamFrameStatus {
    pub fn from_i32(x: i32) -> Option<Self> {
        match x {
            0 => Some(Self::Complete),
            1 => Some(Self::Idle),
            2 => Some(Self::Blank), 
            3 => Some(Self::Stopped),
            _ => None
        }
    }
}

pub(crate) struct CGDisplayStream{
    stream_ref: CGDisplayStreamRef,
    callback_block: RcBlock<(i32, u64, IOSurfaceRef, CGDisplayStreamUpdateRef), ()>,
}

impl CGDisplayStream {
    pub fn new(callback: impl Fn(CGDisplayStreamFrameStatus, Duration, IOSurface) + 'static, display_id: u32, size: (usize, usize), pixel_format: SCStreamPixelFormat, options_dict: NSDictionary, dispatch_queue: DispatchQueue) -> Self {
        println!("CGDisplayStream::new(..)");
        let absolute_time_start = RefCell::new(None);
        let callback_block = ConcreteBlock::new(move |status: i32, display_time: u64, iosurface_ref: IOSurfaceRef, stream_update_ref: CGDisplayStreamUpdateRef| {
            println!("CGDisplayStream callback_block");
            if let Some(status) = CGDisplayStreamFrameStatus::from_i32(status) {
                let relative_time = if let Some(absolute_time_start) = *absolute_time_start.borrow() {
                    display_time - absolute_time_start
                } else {
                    *absolute_time_start.borrow_mut() = Some(display_time);
                    0
                };
                unsafe {
                    let mut timebase_info: mach_timebase_info_data_t = Default::default();
                    mach_timebase_info(&mut timebase_info as *mut _);
                    let time_ns = ((relative_time as u128 * timebase_info.numer as u128) / timebase_info.denom as u128);
                    let time = Duration::from_nanos(time_ns as u64);
                    let io_surface = IOSurface::from_ref_unretained(iosurface_ref);
                    (callback)(status, time, io_surface);
                }
            }
        }).copy();
        unsafe {
            let pixel_format = pixel_format.to_ostype();
            let display_id = CGMainDisplayID();
            let stream_ref = CGDisplayStreamCreateWithDispatchQueue(display_id, size.0, size.1, pixel_format.as_i32(), std::ptr::null_mut(), dispatch_queue.0, &*callback_block as *const _ as *const c_void);
            println!("CGDisplayStreamCreateWithDispatchQueue(display_id: {}, output_width: {}, output_height: {}, pixel_format: {:?}, options_dict: {:?}, dispatch_queue: {:?}, callback_block: {:?}): return value: {:?}", display_id, size.0, size.1, pixel_format, options_dict.0, dispatch_queue.0, &callback_block as *const _, stream_ref);
            Self {
                stream_ref,
                callback_block
            }
        }
    }

    pub fn start(&self) -> Result<(), ()> {
        let error_code = unsafe { CGDisplayStreamStart(self.stream_ref) };
        println!("CGDisplayStreamStart({:?}) return value: {:?}", self.stream_ref, error_code);
        if error_code == 0 {
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn stop(&self) -> Result<(), ()> {
        let error_code = unsafe { CGDisplayStreamStop(self.stream_ref) };
        if error_code == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

impl Clone for CGDisplayStream {
    fn clone(&self) -> Self {
        unsafe {
            CFRetain(self.stream_ref);
        }
        CGDisplayStream {
            stream_ref: self.stream_ref,
            callback_block: self.callback_block.clone()
        }
    }
}

impl Drop for CGDisplayStream {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.stream_ref);
        }
    }
}

pub(crate) struct IOSurface(IOSurfaceRef);

impl IOSurface {
    fn from_ref_unretained(r: IOSurfaceRef) -> Self {
        unsafe { CFRetain(r); }
        Self(r)
    }
}

impl Drop for IOSurface {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}

/*
kCFNumberSInt8Type = 1,
kCFNumberSInt16Type = 2,
kCFNumberSInt32Type = 3,
kCFNumberSInt64Type = 4,
kCFNumberFloat32Type = 5,
kCFNumberFloat64Type = 6,	/* 64-bit IEEE 754 */
/* Basic C types */
kCFNumberCharType = 7,
kCFNumberShortType = 8,
kCFNumberIntType = 9,
kCFNumberLongType = 10,
kCFNumberLongLongType = 11,
kCFNumberFloatType = 12,
kCFNumberDoubleType = 13,
/* Other */
kCFNumberCFIndexType = 14,
*/

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum CFNumberType {
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Char,
    Short,
    Int,
    Long,
    LongLong,
    Float,
    Double,
    CFIndex,
    NSInteger,
    CGFloat,
}

impl CFNumberType {
    pub(crate) fn to_isize(self) -> isize {
        match self {
            Self::I8 => 1,
            Self::I16 => 2,
            Self::I32 => 3,
            Self::I64 => 4,
            Self::F32 => 5,
            Self::F64 => 6,
            Self::Char => 7,
            Self::Short => 8,
            Self::Int => 9,
            Self::Long => 10,
            Self::LongLong => 11,
            Self::Float => 12,
            Self::Double => 13,
            Self::CFIndex => 14,
            Self::NSInteger => 15,
            Self::CGFloat => 16,
        }
    }

    pub(crate) fn from_i32(x: i32) -> Option<Self> {
        Some(match x {
            1 => Self::I8,
            2 => Self::I16,
            3 => Self::I32,
            4 => Self::I64,
            5 => Self::F32,
            6 => Self::F64,
            7 => Self::Char,
            8 => Self::Short,
            9 => Self::Int,
            10 => Self::Long,
            11 => Self::LongLong,
            12 => Self::Float,
            13 => Self::Double,
            14 => Self::CFIndex,
            15 => Self::NSInteger,
            16 => Self::CGFloat,
            _ => return None,
        })
    }
}

#[repr(C)]
pub(crate) struct CFNumber(pub(crate) CFNumberRef);

impl CFNumber {
    pub fn new_f32(x: f32) -> Self {
        unsafe {
            let r = CFNumberCreate(kCFAllocatorNull, CFNumberType::F32.to_isize(), &x as *const f32 as *const c_void);
            Self(r)
        }
    }

    pub fn new_i32(x: i32) -> Self {
        unsafe {
            let r = CFNumberCreate(kCFAllocatorNull, CFNumberType::I32.to_isize(), &x as *const i32 as *const c_void);
            Self(r)
        }
    }
}

impl Clone for CFNumber {
    fn clone(&self) -> Self {
        unsafe { CFRetain(self.0); }
        CFNumber(self.0)
    }
}

impl Drop for CFNumber {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}

pub struct NSNumber(pub(crate) *mut Object);

impl NSNumber {
    pub(crate) fn from_id_unretained(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    pub(crate) fn from_id_retained(id: *mut Object) -> Self {
        Self(id)
    }

    pub(crate) fn new_isize(x: isize) -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(NSNumber), numberWithInteger: x];
            Self(id)
        }
    }

    pub(crate) fn new_f32(x: f32) -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(NSNumber), numberWithFloat: x];
            Self(id)
        }
    }
}

impl Clone for NSNumber {
    fn clone(&self) -> Self {
        Self::from_id_unretained(self.0)
    }
}

impl Drop for NSNumber {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}

pub struct NSApplication(*mut Object);

impl NSApplication {
    fn from_id_unretained(id: *mut Object) -> Self {
        unsafe { let _: () = msg_send![id, retain]; }
        Self(id)
    }

    fn from_id_retained(id: *mut Object) -> Self {
        Self(id)
    }

    pub fn shared() -> Self {
        unsafe {
            let id: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            Self::from_id_unretained(id)
        }
    }

    pub fn run(&self) {
        unsafe {
            let _: () = msg_send![self.0, run];
        }
    }
}

impl Clone for NSApplication {
    fn clone(&self) -> Self {
        Self::from_id_unretained(self.0)
    }
}

impl Drop for NSApplication {
    fn drop(&mut self) {
        unsafe { let _: () = msg_send![self.0, release]; }
    }
}