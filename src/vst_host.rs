//! Minimal VST3 host: loads plugins, runs them on the guitar signal and
//! opens their editors in native Win32 windows.
//!
//! Threading model:
//! - Loading, editors, state save/restore and activation happen on the UI thread.
//! - `RtChain` is handed to the audio thread through `ChainSlot` (a mutex the
//!   audio callback only `try_lock`s, so it never blocks).

#![allow(non_snake_case, unsafe_op_in_unsafe_fn)]

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

/// Largest block handed to a plugin. Bigger host buffers are split.
pub const MAX_BLOCK: usize = 2048;

const SPEAKER_MONO: SpeakerArrangement = 1 << 19; // kSpeakerM
const SPEAKER_STEREO: SpeakerArrangement = 3; // kSpeakerL | kSpeakerR

fn ok(r: tresult) -> bool {
    r == kResultOk
}

fn guid_eq(a: *const TUID, b: &[u8; 16]) -> bool {
    if a.is_null() {
        return false;
    }
    unsafe { &*(a as *const [u8; 16]) == b }
}

fn copy_wide(src: &str, dst: &mut [TChar]) {
    let mut n = 0;
    for (s, d) in src.encode_utf16().zip(dst.iter_mut()) {
        *d = s as TChar;
        n += 1;
    }
    let last = dst.len() - 1;
    dst[n.min(last)] = 0;
}

fn cstr_from(buf: &[char8]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

// ---------------------------------------------------------------------------
// Host-side COM objects
// ---------------------------------------------------------------------------

struct HostApp;

impl Class for HostApp {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        copy_wide("Guitar To Discord", &mut *name);
        kResultOk
    }

    unsafe fn createInstance(&self, cid: *mut TUID, _iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        if obj.is_null() {
            return kInvalidArgument;
        }
        if guid_eq(cid, &IMessage::IID) {
            let msg = ComWrapper::new(Message::default());
            *obj = msg.to_com_ptr::<IMessage>().unwrap().into_raw() as *mut c_void;
            return kResultOk;
        }
        if guid_eq(cid, &IAttributeList::IID) {
            let list = ComWrapper::new(AttrList::default());
            *obj = list.to_com_ptr::<IAttributeList>().unwrap().into_raw() as *mut c_void;
            return kResultOk;
        }
        *obj = ptr::null_mut();
        kResultFalse
    }
}

#[derive(Clone)]
enum Attr {
    Int(i64),
    Float(f64),
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

#[derive(Default)]
struct AttrList {
    map: RefCell<HashMap<String, Attr>>,
}

impl Class for AttrList {
    type Interfaces = (IAttributeList,);
}

unsafe fn attr_key(id: IAttributeList_::AttrID) -> Option<String> {
    if id.is_null() {
        None
    } else {
        Some(CStr::from_ptr(id).to_string_lossy().into_owned())
    }
}

impl IAttributeListTrait for AttrList {
    unsafe fn setInt(&self, id: IAttributeList_::AttrID, value: int64) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        self.map.borrow_mut().insert(k, Attr::Int(value));
        kResultOk
    }
    unsafe fn getInt(&self, id: IAttributeList_::AttrID, value: *mut int64) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        match self.map.borrow().get(&k) {
            Some(Attr::Int(v)) if !value.is_null() => {
                *value = *v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setFloat(&self, id: IAttributeList_::AttrID, value: f64) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        self.map.borrow_mut().insert(k, Attr::Float(value));
        kResultOk
    }
    unsafe fn getFloat(&self, id: IAttributeList_::AttrID, value: *mut f64) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        match self.map.borrow().get(&k) {
            Some(Attr::Float(v)) if !value.is_null() => {
                *value = *v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setString(&self, id: IAttributeList_::AttrID, string: *const TChar) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        let mut v = Vec::new();
        if !string.is_null() {
            let mut i = 0;
            while *string.add(i) != 0 {
                v.push(*string.add(i) as u16);
                i += 1;
            }
        }
        self.map.borrow_mut().insert(k, Attr::Str(v));
        kResultOk
    }
    unsafe fn getString(&self, id: IAttributeList_::AttrID, string: *mut TChar, sizeInBytes: uint32) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        match self.map.borrow().get(&k) {
            Some(Attr::Str(v)) if !string.is_null() => {
                let cap = (sizeInBytes as usize / 2).max(1);
                let n = v.len().min(cap - 1);
                for (i, c) in v.iter().take(n).enumerate() {
                    *string.add(i) = *c as TChar;
                }
                *string.add(n) = 0;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setBinary(&self, id: IAttributeList_::AttrID, data: *const c_void, sizeInBytes: uint32) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        let v = if data.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(data as *const u8, sizeInBytes as usize).to_vec()
        };
        self.map.borrow_mut().insert(k, Attr::Bin(v));
        kResultOk
    }
    unsafe fn getBinary(&self, id: IAttributeList_::AttrID, data: *mut *const c_void, sizeInBytes: *mut uint32) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        // Pointer stays valid as long as the entry is not replaced, like the SDK.
        match self.map.borrow().get(&k) {
            Some(Attr::Bin(v)) if !data.is_null() && !sizeInBytes.is_null() => {
                *data = v.as_ptr() as *const c_void;
                *sizeInBytes = v.len() as uint32;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

struct Message {
    id: RefCell<CString>,
    attrs: ComWrapper<AttrList>,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            id: RefCell::new(CString::default()),
            attrs: ComWrapper::new(AttrList::default()),
        }
    }
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.borrow().as_ptr()
    }
    unsafe fn setMessageID(&self, id: FIDString) {
        if !id.is_null() {
            *self.id.borrow_mut() = CStr::from_ptr(id).to_owned();
        }
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attrs.as_com_ref::<IAttributeList>().unwrap().as_ptr()
    }
}

/// Growable in-memory IBStream (plugin state save/restore).
#[derive(Default)]
struct MemStream {
    data: UnsafeCell<Vec<u8>>,
    pos: Cell<usize>,
}

impl Class for MemStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for MemStream {
    unsafe fn read(&self, buffer: *mut c_void, numBytes: int32, numBytesRead: *mut int32) -> tresult {
        let data = &*self.data.get();
        let pos = self.pos.get();
        let n = (numBytes.max(0) as usize).min(data.len().saturating_sub(pos));
        if n > 0 && !buffer.is_null() {
            ptr::copy_nonoverlapping(data.as_ptr().add(pos), buffer as *mut u8, n);
        }
        self.pos.set(pos + n);
        if !numBytesRead.is_null() {
            *numBytesRead = n as int32;
        }
        kResultOk
    }
    unsafe fn write(&self, buffer: *mut c_void, numBytes: int32, numBytesWritten: *mut int32) -> tresult {
        let data = &mut *self.data.get();
        let pos = self.pos.get();
        let n = numBytes.max(0) as usize;
        if data.len() < pos + n {
            data.resize(pos + n, 0);
        }
        if n > 0 && !buffer.is_null() {
            ptr::copy_nonoverlapping(buffer as *const u8, data.as_mut_ptr().add(pos), n);
        }
        self.pos.set(pos + n);
        if !numBytesWritten.is_null() {
            *numBytesWritten = n as int32;
        }
        kResultOk
    }
    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let len = (*self.data.get()).len() as i64;
        let base = match mode as u32 {
            m if m == IBStream_::IStreamSeekMode_::kIBSeekSet as u32 => 0,
            m if m == IBStream_::IStreamSeekMode_::kIBSeekCur as u32 => self.pos.get() as i64,
            _ => len,
        };
        let np = (base + pos).clamp(0, len.max(base + pos).max(0));
        self.pos.set(np as usize);
        if !result.is_null() {
            *result = np;
        }
        kResultOk
    }
    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if !pos.is_null() {
            *pos = self.pos.get() as int64;
        }
        kResultOk
    }
}

fn new_stream(bytes: Vec<u8>) -> ComWrapper<MemStream> {
    ComWrapper::new(MemStream {
        data: UnsafeCell::new(bytes),
        pos: Cell::new(0),
    })
}

fn stream_ptr(s: &ComWrapper<MemStream>) -> *mut IBStream {
    s.as_com_ref::<IBStream>().unwrap().as_ptr()
}

fn stream_bytes(s: &ComWrapper<MemStream>) -> Vec<u8> {
    unsafe { (*s.data.get()).clone() }
}

/// Parameter edits coming from a plugin editor, waiting for the audio thread.
#[derive(Default)]
pub struct ParamQueue {
    pending: Mutex<Vec<(ParamID, ParamValue)>>,
}

struct ComponentHandler {
    queue: Arc<ParamQueue>,
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, id: ParamID, valueNormalized: ParamValue) -> tresult {
        if let Ok(mut p) = self.queue.pending.lock() {
            if let Some(slot) = p.iter_mut().find(|(pid, _)| *pid == id) {
                slot.1 = valueNormalized;
            } else if p.len() < 512 {
                p.push((id, valueNormalized));
            }
        }
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, _flags: int32) -> tresult {
        kResultOk
    }
}

// --- parameter changes handed to process() (audio thread, preallocated) ---

struct ValueQueue {
    id: Cell<ParamID>,
    value: Cell<ParamValue>,
}

impl Class for ValueQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ValueQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }
    unsafe fn getPointCount(&self) -> int32 {
        1
    }
    unsafe fn getPoint(&self, index: int32, sampleOffset: *mut int32, value: *mut ParamValue) -> tresult {
        if index != 0 || sampleOffset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        *sampleOffset = 0;
        *value = self.value.get();
        kResultOk
    }
    unsafe fn addPoint(&self, _sampleOffset: int32, value: ParamValue, index: *mut int32) -> tresult {
        self.value.set(value);
        if !index.is_null() {
            *index = 0;
        }
        kResultOk
    }
}

const MAX_PARAM_CHANGES: usize = 128;

struct ParamChanges {
    queues: Vec<ComWrapper<ValueQueue>>,
    used: Cell<usize>,
}

impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.used.get() as int32
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        let i = index as usize;
        if index < 0 || i >= self.used.get() {
            return ptr::null_mut();
        }
        self.queues[i].as_com_ref::<IParamValueQueue>().unwrap().as_ptr()
    }
    unsafe fn addParameterData(&self, _id: *const ParamID, _index: *mut int32) -> *mut IParamValueQueue {
        ptr::null_mut()
    }
}

// ---------------------------------------------------------------------------
// Module (the plugin DLL)
// ---------------------------------------------------------------------------

struct Module {
    factory: Option<ComPtr<IPluginFactory>>,
    lib: Option<libloading::Library>,
}

impl Drop for Module {
    fn drop(&mut self) {
        // Release the factory before ExitDll / unloading the library.
        unsafe {
            self.factory = None;
            if let Some(lib) = self.lib.take() {
                if let Ok(exit) = lib.get::<unsafe extern "system" fn() -> bool>(b"ExitDll\0") {
                    exit();
                }
                drop(lib);
            }
        }
    }
}

/// Path of the binary inside a `.vst3` bundle (or the file itself).
pub fn binary_path(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    let arch = if cfg!(target_arch = "aarch64") { "arm64-win" } else { "x86_64-win" };
    let dir = path.join("Contents").join(arch);
    let entries = std::fs::read_dir(&dir).with_context(|| format!("no {arch} folder in {}", path.display()))?;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().map(|x| x.eq_ignore_ascii_case("vst3")).unwrap_or(false) && p.is_file() {
            return Ok(p);
        }
    }
    bail!("no 64-bit binary in {}", path.display())
}

fn load_module(path: &Path) -> Result<Arc<Module>> {
    let bin = binary_path(path)?;
    unsafe {
        let lib = libloading::Library::new(&bin).with_context(|| format!("cannot load {}", bin.display()))?;
        if let Ok(init) = lib.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0") {
            init();
        }
        let get = lib
            .get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0")
            .map_err(|_| anyhow!("not a VST3 plugin (no GetPluginFactory)"))?;
        let factory = ComPtr::from_raw(get()).ok_or_else(|| anyhow!("plugin returned no factory"))?;
        Ok(Arc::new(Module { factory: Some(factory), lib: Some(lib) }))
    }
}

// ---------------------------------------------------------------------------
// Plugin instance (UI thread)
// ---------------------------------------------------------------------------

pub struct PluginInstance {
    pub name: String,
    pub path: PathBuf,
    pub enabled: Arc<AtomicBool>,
    pub in_ch: usize,
    pub out_ch: usize,
    in_bus_ch: Vec<usize>,
    out_bus_ch: Vec<usize>,
    editor: Option<EditorWindow>,
    queue: Arc<ParamQueue>,
    _handler: ComWrapper<ComponentHandler>,
    connection: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    processor: ComPtr<IAudioProcessor>,
    component: ComPtr<IComponent>,
    active: bool,
    _host: ComWrapper<HostApp>,
    _module: Arc<Module>,
}

impl PluginInstance {
    pub fn load(path: &Path, state: Option<&str>, ctrl_state: Option<&str>) -> Result<PluginInstance> {
        let module = load_module(path)?;
        let host = ComWrapper::new(HostApp);
        let host_unknown = host.as_com_ref::<IHostApplication>().unwrap().as_ptr() as *mut FUnknown;
        unsafe {
            let factory = module.factory.as_ref().unwrap();
            // Pick the first audio effect class.
            let mut class: Option<PClassInfo> = None;
            for i in 0..factory.countClasses() {
                let mut info: PClassInfo = std::mem::zeroed();
                if ok(factory.getClassInfo(i, &mut info)) && cstr_from(&info.category) == "Audio Module Class" {
                    class = Some(info);
                    break;
                }
            }
            let class = class.ok_or_else(|| anyhow!("no audio effect in this plugin"))?;
            let name = cstr_from(&class.name);

            let mut obj: *mut c_void = ptr::null_mut();
            let r = factory.createInstance(
                class.cid.as_ptr(),
                IComponent::IID.as_ptr() as FIDString,
                &mut obj,
            );
            let component = if ok(r) { ComPtr::from_raw(obj as *mut IComponent) } else { None }
                .ok_or_else(|| anyhow!("cannot create the plugin"))?;
            if !ok(component.initialize(host_unknown)) {
                bail!("plugin refused to initialize");
            }
            let processor = component
                .cast::<IAudioProcessor>()
                .ok_or_else(|| anyhow!("not an audio processor"))?;

            // Controller: same object, or a separate class.
            let mut separate_controller = false;
            let controller = match component.cast::<IEditController>() {
                Some(c) => Some(c),
                None => {
                    let mut cid: TUID = [0; 16];
                    if ok(component.getControllerClassId(&mut cid)) {
                        let mut o: *mut c_void = ptr::null_mut();
                        let r = factory.createInstance(cid.as_ptr(), IEditController::IID.as_ptr() as FIDString, &mut o);
                        match if ok(r) { ComPtr::from_raw(o as *mut IEditController) } else { None } {
                            Some(c) if ok(c.initialize(host_unknown)) => {
                                separate_controller = true;
                                Some(c)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
            };

            let queue = Arc::new(ParamQueue::default());
            let handler = ComWrapper::new(ComponentHandler { queue: queue.clone() });
            let mut connection = None;
            if let Some(ctrl) = &controller {
                ctrl.setComponentHandler(handler.as_com_ref::<IComponentHandler>().unwrap().as_ptr());
                if separate_controller {
                    if let (Some(a), Some(b)) = (component.cast::<IConnectionPoint>(), ctrl.cast::<IConnectionPoint>()) {
                        a.connect(b.as_ptr());
                        b.connect(a.as_ptr());
                        connection = Some((a, b));
                    }
                }
            }

            // Buses: main in / main out, stereo when possible.
            let n_in = component.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32);
            let n_out = component.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32);
            if n_out < 1 {
                bail!("plugin has no audio output");
            }
            if n_in < 1 {
                bail!("instrument plugins are not supported (no audio input)");
            }
            // Every bus gets buffers (extra ones, e.g. sidechain, get silence):
            // some plugins output nothing when a declared bus is missing.
            let arr = |dir: i32, i: i32| {
                let mut a: SpeakerArrangement = SPEAKER_STEREO;
                processor.getBusArrangement(dir, i, &mut a);
                a
            };
            let kin = BusDirections_::kInput as i32;
            let kout = BusDirections_::kOutput as i32;
            for i in 0..n_in {
                component.activateBus(MediaTypes_::kAudio as i32, kin, i, 1);
            }
            for i in 0..n_out {
                component.activateBus(MediaTypes_::kAudio as i32, kout, i, 1);
            }
            let mut ins: Vec<SpeakerArrangement> = (0..n_in).map(|i| arr(kin, i)).collect();
            let mut outs: Vec<SpeakerArrangement> = (0..n_out).map(|i| arr(kout, i)).collect();
            ins[0] = SPEAKER_STEREO;
            outs[0] = SPEAKER_STEREO;
            if !ok(processor.setBusArrangements(ins.as_mut_ptr(), n_in, outs.as_mut_ptr(), n_out)) {
                ins[0] = SPEAKER_MONO;
                processor.setBusArrangements(ins.as_mut_ptr(), n_in, outs.as_mut_ptr(), n_out);
            }
            let in_bus_ch: Vec<usize> = (0..n_in).map(|i| (arr(kin, i).count_ones() as usize).min(8)).collect();
            let out_bus_ch: Vec<usize> = (0..n_out).map(|i| (arr(kout, i).count_ones() as usize).min(8)).collect();
            let in_ch = in_bus_ch[0].max(1);
            let out_ch = out_bus_ch[0].max(1);

            if !ok(processor.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as i32)) {
                bail!("plugin cannot process 32-bit audio");
            }

            let inst = PluginInstance {
                name,
                path: path.to_path_buf(),
                enabled: Arc::new(AtomicBool::new(true)),
                in_ch,
                out_ch,
                in_bus_ch,
                out_bus_ch,
                editor: None,
                queue,
                _handler: handler,
                connection,
                controller,
                separate_controller,
                processor,
                component,
                active: false,
                _host: host,
                _module: module,
            };
            inst.restore_state(state, ctrl_state);
            Ok(inst)
        }
    }

    fn restore_state(&self, state: Option<&str>, ctrl_state: Option<&str>) {
        let b64 = base64::engine::general_purpose::STANDARD;
        unsafe {
            if let Some(bytes) = state.and_then(|s| b64.decode(s).ok()) {
                let s = new_stream(bytes.clone());
                self.component.setState(stream_ptr(&s));
                if let Some(c) = &self.controller {
                    let s2 = new_stream(bytes);
                    c.setComponentState(stream_ptr(&s2));
                }
            } else if let Some(c) = &self.controller {
                // Fresh plugin: mirror the processor's defaults into the controller.
                let s = new_stream(Vec::new());
                if ok(self.component.getState(stream_ptr(&s))) {
                    let s2 = new_stream(stream_bytes(&s));
                    c.setComponentState(stream_ptr(&s2));
                }
            }
            if let (Some(c), Some(bytes)) = (&self.controller, ctrl_state.and_then(|s| b64.decode(s).ok())) {
                let s = new_stream(bytes);
                c.setState(stream_ptr(&s));
            }
        }
    }

    /// Serialized (component, controller) state, base64.
    pub fn save_state(&self) -> (Option<String>, Option<String>) {
        let b64 = base64::engine::general_purpose::STANDARD;
        unsafe {
            let s = new_stream(Vec::new());
            let comp = ok(self.component.getState(stream_ptr(&s))).then(|| b64.encode(stream_bytes(&s)));
            let ctrl = self.controller.as_ref().and_then(|c| {
                let s = new_stream(Vec::new());
                ok(c.getState(stream_ptr(&s))).then(|| b64.encode(stream_bytes(&s)))
            });
            (comp, ctrl)
        }
    }

    fn activate(&mut self, sample_rate: f64) {
        unsafe {
            if self.active {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            let mut setup = ProcessSetup {
                processMode: ProcessModes_::kRealtime as i32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
                maxSamplesPerBlock: MAX_BLOCK as i32,
                sampleRate: sample_rate,
            };
            self.processor.setupProcessing(&mut setup);
            self.component.setActive(1);
            self.processor.setProcessing(1);
            self.active = true;
        }
    }

    fn deactivate(&mut self) {
        if self.active {
            unsafe {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            self.active = false;
        }
    }

    fn rt(&self, sample_rate: f64) -> RtPlugin {
        let mk = |n: usize| (0..n).map(|_| vec![0f32; MAX_BLOCK]).collect::<Vec<_>>();
        let queues = (0..MAX_PARAM_CHANGES)
            .map(|_| ComWrapper::new(ValueQueue { id: Cell::new(0), value: Cell::new(0.0) }))
            .collect();
        let mut ctx: ProcessContext = unsafe { std::mem::zeroed() };
        ctx.sampleRate = sample_rate;
        ctx.tempo = 120.0;
        ctx.timeSigNumerator = 4;
        ctx.timeSigDenominator = 4;
        ctx.state = (ProcessContext_::StatesAndFlags_::kTempoValid | ProcessContext_::StatesAndFlags_::kTimeSigValid) as u32;
        RtPlugin {
            processor: self.processor.clone(),
            queue: self.queue.clone(),
            enabled: self.enabled.clone(),
            in_ch: self.in_ch,
            out_ch: self.out_ch,
            in_bufs: mk(self.in_bus_ch.iter().sum::<usize>().max(1)),
            out_bufs: mk(self.out_bus_ch.iter().sum::<usize>().max(1)),
            in_ptrs: Vec::with_capacity(16),
            out_ptrs: Vec::with_capacity(16),
            in_bus_ch: self.in_bus_ch.clone(),
            out_bus_ch: self.out_bus_ch.clone(),
            in_buses: Vec::with_capacity(self.in_bus_ch.len()),
            out_buses: Vec::with_capacity(self.out_bus_ch.len()),
            changes: ComWrapper::new(ParamChanges { queues, used: Cell::new(0) }),
            ctx,
            pending: Vec::with_capacity(MAX_PARAM_CHANGES),
        }
    }

    pub fn has_editor(&self) -> bool {
        self.controller.is_some()
    }

    pub fn editor_open(&self) -> bool {
        self.editor.is_some()
    }

    pub fn open_editor(&mut self) -> Result<()> {
        if let Some(ed) = &self.editor {
            ed.focus();
            return Ok(());
        }
        let ctrl = self.controller.as_ref().ok_or_else(|| anyhow!("this plugin has no editor"))?;
        let view = unsafe { ComPtr::from_raw(ctrl.createView(ViewType::kEditor)) }.ok_or_else(|| anyhow!("this plugin has no editor"))?;
        self.editor = Some(EditorWindow::open(&self.name, view)?);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn close_editor(&mut self) {
        self.editor = None;
    }

    /// Close editors whose window the user closed.
    pub fn poll_editor(&mut self) {
        if self.editor.as_ref().map(|e| e.closed()).unwrap_or(false) {
            self.editor = None;
        }
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        self.editor = None;
        self.deactivate();
        unsafe {
            if let Some((a, b)) = self.connection.take() {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
            if let Some(c) = &self.controller {
                c.setComponentHandler(ptr::null_mut());
                if self.separate_controller {
                    c.terminate();
                }
            }
            self.controller = None;
            self.component.terminate();
        }
    }
}

// ---------------------------------------------------------------------------
// Real-time side
// ---------------------------------------------------------------------------

pub struct RtPlugin {
    processor: ComPtr<IAudioProcessor>,
    queue: Arc<ParamQueue>,
    enabled: Arc<AtomicBool>,
    in_ch: usize,
    out_ch: usize,
    in_bufs: Vec<Vec<f32>>,
    out_bufs: Vec<Vec<f32>>,
    in_ptrs: Vec<*mut f32>,
    out_ptrs: Vec<*mut f32>,
    in_bus_ch: Vec<usize>,
    out_bus_ch: Vec<usize>,
    in_buses: Vec<AudioBusBuffers>,
    out_buses: Vec<AudioBusBuffers>,
    changes: ComWrapper<ParamChanges>,
    ctx: ProcessContext,
    pending: Vec<(ParamID, ParamValue)>,
}

impl RtPlugin {
    fn process_block(&mut self, buf: &mut [f32]) {
        let n = buf.len();
        for (c, b) in self.in_bufs.iter_mut().enumerate() {
            if c < self.in_ch {
                b[..n].copy_from_slice(buf);
            } else {
                b[..n].fill(0.0); // sidechain / extra inputs: silence
            }
        }
        // Pre-fill the outputs with the dry signal: some plugins skip writing
        // their output when they leave the sound unchanged (they assume the
        // host processes in place).
        for (c, b) in self.out_bufs.iter_mut().enumerate() {
            if c < self.out_ch {
                b[..n].copy_from_slice(buf);
            } else {
                b[..n].fill(0.0);
            }
        }
        // Collect editor edits without ever blocking.
        self.pending.clear();
        if let Ok(mut p) = self.queue.pending.try_lock() {
            self.pending.extend(p.drain(..).take(MAX_PARAM_CHANGES));
        }
        for (i, (id, v)) in self.pending.iter().enumerate() {
            self.changes.queues[i].id.set(*id);
            self.changes.queues[i].value.set(*v);
        }
        self.changes.used.set(self.pending.len());

        self.in_ptrs.clear();
        self.in_ptrs.extend(self.in_bufs.iter_mut().map(|b| b.as_mut_ptr()));
        self.out_ptrs.clear();
        self.out_ptrs.extend(self.out_bufs.iter_mut().map(|b| b.as_mut_ptr()));
        unsafe {
            self.in_buses.clear();
            let mut off = 0;
            for &ch in &self.in_bus_ch {
                let mut b: AudioBusBuffers = std::mem::zeroed();
                b.numChannels = ch as i32;
                b.__field0.channelBuffers32 = self.in_ptrs.as_mut_ptr().add(off);
                self.in_buses.push(b);
                off += ch;
            }
            self.out_buses.clear();
            let mut off = 0;
            for &ch in &self.out_bus_ch {
                let mut b: AudioBusBuffers = std::mem::zeroed();
                b.numChannels = ch as i32;
                b.__field0.channelBuffers32 = self.out_ptrs.as_mut_ptr().add(off);
                self.out_buses.push(b);
                off += ch;
            }

            let mut data: ProcessData = std::mem::zeroed();
            data.processMode = ProcessModes_::kRealtime as i32;
            data.symbolicSampleSize = SymbolicSampleSizes_::kSample32 as i32;
            data.numSamples = n as i32;
            data.numInputs = self.in_buses.len() as i32;
            data.numOutputs = self.out_buses.len() as i32;
            data.inputs = self.in_buses.as_mut_ptr();
            data.outputs = self.out_buses.as_mut_ptr();
            data.inputParameterChanges = self.changes.as_com_ref::<IParameterChanges>().unwrap().as_ptr();
            data.processContext = &mut self.ctx;

            if !ok(self.processor.process(&mut data)) {
                return; // leave the signal untouched
            }
        }
        self.ctx.projectTimeSamples += n as i64;
        self.ctx.continousTimeSamples += n as i64;
        let scale = 1.0 / self.out_ch.min(2) as f32;
        for (i, s) in buf.iter_mut().enumerate() {
            let mut v = 0.0;
            for c in 0..self.out_ch.min(2) {
                v += self.out_bufs[c][i];
            }
            let v = v * scale;
            *s = if v.is_finite() { v } else { 0.0 };
        }
    }
}

pub struct RtChain {
    plugins: Vec<RtPlugin>,
}

// The processors are only touched by the audio thread while installed in the
// slot; the UI thread takes the chain out before deactivating plugins.
unsafe impl Send for RtChain {}

impl RtChain {
    pub fn process(&mut self, buf: &mut [f32]) {
        for chunk in buf.chunks_mut(MAX_BLOCK) {
            for p in self.plugins.iter_mut() {
                if p.enabled.load(Ordering::Relaxed) {
                    p.process_block(chunk);
                }
            }
        }
    }
}

pub type ChainSlot = Mutex<Option<RtChain>>;

/// Owns the loaded plugins (UI thread) and feeds the audio thread.
pub struct PluginHost {
    pub slot: Arc<ChainSlot>,
    pub instances: Vec<Result<PluginInstance, String>>,
    rate: Option<f64>,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self {
            slot: Arc::new(Mutex::new(None)),
            instances: Vec::new(),
            rate: None,
        }
    }
}

impl PluginHost {
    /// Make `instances` match `entries`, keeping already-loaded plugins.
    pub fn sync(&mut self, entries: &[crate::config::ChainEntry]) {
        let rate = self.rate;
        self.stop();
        let mut old: Vec<Option<Result<PluginInstance, String>>> = std::mem::take(&mut self.instances).into_iter().map(Some).collect();
        for e in entries {
            let reuse = old.iter_mut().position(|o| matches!(o, Some(Ok(i)) if i.path == e.path));
            let inst = match reuse {
                Some(i) => old[i].take().unwrap(),
                None => PluginInstance::load(&e.path, e.state.as_deref(), e.ctrl_state.as_deref()).map_err(|err| format!("{err:#}")),
            };
            if let Ok(i) = &inst {
                i.enabled.store(e.enabled, Ordering::Relaxed);
            }
            self.instances.push(inst);
        }
        drop(old);
        if let Some(r) = rate {
            self.start(r);
        }
    }

    /// Activate every plugin at `sample_rate` and hand the chain to the audio thread.
    pub fn start(&mut self, sample_rate: f64) {
        self.stop();
        let mut rts = Vec::new();
        for inst in self.instances.iter_mut().flatten() {
            inst.activate(sample_rate);
            rts.push(inst.rt(sample_rate));
        }
        if let Ok(mut s) = self.slot.lock() {
            *s = Some(RtChain { plugins: rts });
        }
        self.rate = Some(sample_rate);
    }

    pub fn stop(&mut self) {
        if let Ok(mut s) = self.slot.lock() {
            *s = None; // audio thread no longer sees the processors
        }
        for inst in self.instances.iter_mut().flatten() {
            inst.deactivate();
        }
        self.rate = None;
    }

    pub fn save_states(&self, entries: &mut [crate::config::ChainEntry]) {
        for (e, inst) in entries.iter_mut().zip(self.instances.iter()) {
            if let Ok(i) = inst {
                let (s, c) = i.save_state();
                if s.is_some() {
                    e.state = s;
                }
                if c.is_some() {
                    e.ctrl_state = c;
                }
            }
        }
    }

    pub fn poll_editors(&mut self) {
        for inst in self.instances.iter_mut().flatten() {
            inst.poll_editor();
        }
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.stop();
        self.instances.clear();
    }
}

// ---------------------------------------------------------------------------
// Editor windows (Win32)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use super::*;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    pub struct WinData {
        pub closed: AtomicBool,
        pub view: *mut IPlugView,
        pub resizable: bool,
        pub in_frame_resize: Cell<bool>,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    const CLASS: &str = "GuitarToDiscordPluginEditor";

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WinData;
        match msg {
            WM_CLOSE => {
                if !data.is_null() {
                    (*data).closed.store(true, Ordering::Relaxed);
                }
                ShowWindow(hwnd, SW_HIDE);
                0
            }
            WM_SIZE => {
                if !data.is_null() && (*data).resizable && !(*data).in_frame_resize.get() && !(*data).view.is_null() {
                    let mut rc: RECT = std::mem::zeroed();
                    GetClientRect(hwnd, &mut rc);
                    let mut vr = ViewRect { left: 0, top: 0, right: rc.right - rc.left, bottom: rc.bottom - rc.top };
                    let v = vst3::ComRef::from_raw_unchecked((*data).view);
                    v.onSize(&mut vr);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    pub unsafe fn register() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let name = wide(CLASS);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: GetModuleHandleW(ptr::null()),
                hIcon: ptr::null_mut(),
                hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
                hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
                lpszMenuName: ptr::null(),
                lpszClassName: name.as_ptr(),
                hIconSm: ptr::null_mut(),
            };
            RegisterClassExW(&wc);
            std::mem::forget(name);
        });
    }

    pub fn style(resizable: bool) -> u32 {
        let base = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        if resizable {
            base | WS_THICKFRAME | WS_MAXIMIZEBOX
        } else {
            base
        }
    }

    pub unsafe fn outer_size(w: i32, h: i32, resizable: bool) -> (i32, i32) {
        let mut rc = RECT { left: 0, top: 0, right: w, bottom: h };
        AdjustWindowRectEx(&mut rc, style(resizable), 0, 0);
        (rc.right - rc.left, rc.bottom - rc.top)
    }

    pub unsafe fn create(title: &str, w: i32, h: i32, resizable: bool) -> HWND {
        register();
        let (ow, oh) = outer_size(w, h, resizable);
        let class = wide(CLASS);
        let title = wide(title);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            style(resizable),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            ow,
            oh,
            ptr::null_mut(),
            ptr::null_mut(),
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        );
        hwnd
    }

    pub unsafe fn set_data(hwnd: HWND, data: *const WinData) {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize);
    }

    pub unsafe fn show(hwnd: HWND) {
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }

    pub unsafe fn resize(hwnd: HWND, w: i32, h: i32, resizable: bool) {
        let (ow, oh) = outer_size(w, h, resizable);
        SetWindowPos(hwnd, ptr::null_mut(), 0, 0, ow, oh, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }

    pub unsafe fn destroy(hwnd: HWND) {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        DestroyWindow(hwnd);
    }
}

struct PlugFrame {
    hwnd: Cell<*mut c_void>,
    data: Cell<*const win::WinData>,
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, newSize: *mut ViewRect) -> tresult {
        if view.is_null() || newSize.is_null() || self.hwnd.get().is_null() {
            return kInvalidArgument;
        }
        let r = &*newSize;
        let data = self.data.get();
        let resizable = !data.is_null() && (*data).resizable;
        if !data.is_null() {
            (*data).in_frame_resize.set(true);
        }
        win::resize(self.hwnd.get(), r.right - r.left, r.bottom - r.top, resizable);
        if !data.is_null() {
            (*data).in_frame_resize.set(false);
        }
        vst3::ComRef::from_raw_unchecked(view).onSize(newSize);
        kResultOk
    }
}

struct EditorWindow {
    hwnd: *mut c_void,
    view: ComPtr<IPlugView>,
    frame: ComWrapper<PlugFrame>,
    data: Box<win::WinData>,
}

impl EditorWindow {
    fn open(title: &str, view: ComPtr<IPlugView>) -> Result<EditorWindow> {
        unsafe {
            if !ok(view.isPlatformTypeSupported(kPlatformTypeHWND)) {
                bail!("the plugin editor does not support Windows windows");
            }
            let mut rect = ViewRect { left: 0, top: 0, right: 640, bottom: 400 };
            view.getSize(&mut rect);
            let w = (rect.right - rect.left).max(100);
            let h = (rect.bottom - rect.top).max(60);
            let resizable = ok(view.canResize());
            let hwnd = win::create(title, w, h, resizable);
            if hwnd.is_null() {
                bail!("cannot create the editor window");
            }
            let data = Box::new(win::WinData {
                closed: AtomicBool::new(false),
                view: view.as_ptr(),
                resizable,
                in_frame_resize: Cell::new(false),
            });
            win::set_data(hwnd, &*data);
            let frame = ComWrapper::new(PlugFrame {
                hwnd: Cell::new(hwnd),
                data: Cell::new(&*data),
            });
            view.setFrame(frame.as_com_ref::<IPlugFrame>().unwrap().as_ptr());
            if !ok(view.attached(hwnd, kPlatformTypeHWND)) {
                view.setFrame(ptr::null_mut());
                win::destroy(hwnd);
                bail!("the plugin could not open its editor");
            }
            win::show(hwnd);
            Ok(EditorWindow { hwnd, view, frame, data })
        }
    }

    fn closed(&self) -> bool {
        self.data.closed.load(Ordering::Relaxed)
    }

    fn focus(&self) {
        unsafe {
            self.data.closed.store(false, Ordering::Relaxed);
            win::show(self.hwnd);
        }
    }
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        unsafe {
            self.view.removed();
            self.view.setFrame(ptr::null_mut());
            self.frame.hwnd.set(ptr::null_mut());
            win::destroy(self.hwnd);
        }
    }
}
