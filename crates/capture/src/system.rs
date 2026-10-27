//! Machine facts for the local-model recommendation, and a job object that takes sidecar
//! processes down with the app.

/// Installed RAM and the GPUs with their dedicated memory.
#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
    pub ram_bytes: u64,
    pub cpu_threads: u32,
    /// (adapter name, dedicated video memory). Software renderers are left out.
    pub gpus: Vec<(String, u64)>,
}

pub fn system_info() -> SystemInfo {
    SystemInfo {
        ram_bytes: imp::total_ram().unwrap_or(0),
        cpu_threads: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
        gpus: imp::gpus(),
    }
}

/// Child processes assigned to this job are killed when the last handle closes, which happens
/// when the app exits, also when it crashes. Without it a crashed app would leave a sidecar
/// running that holds gigabytes of model in memory.
pub struct ProcessJob(imp::Handle);

// SAFETY: a job handle is a kernel object handle, usable from any thread.
unsafe impl Send for ProcessJob {}
unsafe impl Sync for ProcessJob {}

impl ProcessJob {
    pub fn new() -> Option<Self> {
        imp::create_kill_on_close_job().map(ProcessJob)
    }

    /// Whether the process now belongs to the job.
    pub fn assign(&self, child: &std::process::Child) -> bool {
        imp::assign(self.0, child)
    }
}

impl Drop for ProcessJob {
    fn drop(&mut self) {
        imp::close(self.0);
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    pub type Handle = *mut c_void;

    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[repr(C)]
    struct BasicLimits {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io_counters: [u64; 6],
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub fn total_ram() -> Option<u64> {
        // SAFETY: zeroed is a valid bit pattern for this plain struct; `length` is set as required.
        let mut s: MemoryStatusEx = unsafe { std::mem::zeroed() };
        s.length = std::mem::size_of::<MemoryStatusEx>() as u32;
        // SAFETY: points to a correctly sized, initialized struct.
        (unsafe { GlobalMemoryStatusEx(&mut s) } != 0).then_some(s.total_phys)
    }

    pub fn create_kill_on_close_job() -> Option<Handle> {
        // SAFETY: anonymous job with default security.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return None;
        }
        // SAFETY: zeroed is a valid bit pattern for this plain struct.
        let mut limits: ExtendedLimits = unsafe { std::mem::zeroed() };
        limits.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `limits` matches JOBOBJECT_EXTENDED_LIMIT_INFORMATION and outlives the call.
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&limits as *const ExtendedLimits).cast(),
                std::mem::size_of::<ExtendedLimits>() as u32,
            )
        };
        if ok == 0 {
            close(job);
            return None;
        }
        Some(job)
    }

    pub fn assign(job: Handle, child: &std::process::Child) -> bool {
        // SAFETY: both handles are valid while `job` and `child` live.
        unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) != 0 }
    }

    pub fn close(handle: Handle) {
        // SAFETY: the handle came from CreateJobObjectW and is closed once.
        unsafe { CloseHandle(handle) };
    }

    // ----- DXGI: adapters and their dedicated video memory -----

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);

    const IID_IDXGI_FACTORY1: Guid = Guid(0x770a_ae78, 0xf26f, 0x4dba, [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87]);

    #[repr(C)]
    struct AdapterDesc1 {
        description: [u16; 128],
        vendor_id: u32,
        device_id: u32,
        sub_sys_id: u32,
        revision: u32,
        dedicated_video_memory: usize,
        dedicated_system_memory: usize,
        shared_system_memory: usize,
        luid_low: u32,
        luid_high: i32,
        flags: u32,
    }

    const DXGI_ADAPTER_FLAG_SOFTWARE: u32 = 2;
    const DXGI_ERROR_NOT_FOUND: i32 = 0x887A_0002_u32 as i32;

    // COM vtable slots: IUnknown (0 QueryInterface, 1 AddRef, 2 Release), IDXGIObject (3–6),
    // IDXGIFactory (7–11), IDXGIFactory1::EnumAdapters1 = 12. IDXGIAdapter (7–9),
    // IDXGIAdapter1::GetDesc1 = 10.
    const RELEASE: usize = 2;
    const ENUM_ADAPTERS1: usize = 12;
    const GET_DESC1: usize = 10;

    #[link(name = "dxgi")]
    extern "system" {
        fn CreateDXGIFactory1(riid: *const Guid, factory: *mut *mut c_void) -> i32;
    }

    /// The function in slot `index` of a COM object's vtable.
    ///
    /// SAFETY: `obj` must be a live COM object whose vtable has at least `index + 1` slots.
    unsafe fn slot(obj: *mut c_void, index: usize) -> *const c_void {
        let vtable = *(obj as *const *const *const c_void);
        *vtable.add(index)
    }

    unsafe fn release(obj: *mut c_void) {
        let f: extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(slot(obj, RELEASE));
        f(obj);
    }

    pub fn gpus() -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let mut factory: *mut c_void = std::ptr::null_mut();
        // SAFETY: valid IID and out pointer. The vtable slots follow dxgi.h (IDXGIFactory1,
        // IDXGIAdapter1); every object obtained is released once.
        unsafe {
            if CreateDXGIFactory1(&IID_IDXGI_FACTORY1, &mut factory) < 0 || factory.is_null() {
                return out;
            }
            let enum_adapters: extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 =
                std::mem::transmute(slot(factory, ENUM_ADAPTERS1));
            for i in 0..16 {
                let mut adapter: *mut c_void = std::ptr::null_mut();
                let hr = enum_adapters(factory, i, &mut adapter);
                if hr == DXGI_ERROR_NOT_FOUND || hr < 0 || adapter.is_null() {
                    break;
                }
                let get_desc: extern "system" fn(*mut c_void, *mut AdapterDesc1) -> i32 =
                    std::mem::transmute(slot(adapter, GET_DESC1));
                let mut desc: AdapterDesc1 = std::mem::zeroed();
                if get_desc(adapter, &mut desc) >= 0 && desc.flags & DXGI_ADAPTER_FLAG_SOFTWARE == 0 {
                    let len = desc.description.iter().position(|&c| c == 0).unwrap_or(desc.description.len());
                    let name = String::from_utf16_lossy(&desc.description[..len]).trim().to_string();
                    out.push((name, desc.dedicated_video_memory as u64));
                }
                release(adapter);
            }
            release(factory);
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn struct_layouts_match_the_windows_headers() {
            assert_eq!(std::mem::size_of::<MemoryStatusEx>(), 64);
            assert_eq!(std::mem::size_of::<ExtendedLimits>(), 144);
            assert_eq!(std::mem::size_of::<AdapterDesc1>(), 312);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub type Handle = ();

    pub fn total_ram() -> Option<u64> {
        None
    }
    pub fn gpus() -> Vec<(String, u64)> {
        Vec::new()
    }
    pub fn create_kill_on_close_job() -> Option<Handle> {
        None
    }
    pub fn assign(_job: Handle, _child: &std::process::Child) -> bool {
        false
    }
    pub fn close(_handle: Handle) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_ram_and_threads() {
        let info = system_info();
        eprintln!("{info:?}");
        assert!(info.cpu_threads >= 1);
        if cfg!(windows) {
            assert!(info.ram_bytes > 1 << 30);
        }
    }

    /// A child in the job dies when the job handle closes, as it would when the app exits.
    #[cfg(windows)]
    #[test]
    fn closing_the_job_kills_its_processes() {
        let job = ProcessJob::new().expect("job object");
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        assert!(job.assign(&child));
        assert!(child.try_wait().unwrap().is_none());
        drop(job);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline, "child survived the job");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
