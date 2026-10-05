// Wayland transport adapted from Wawona's MIT-licensed wayland-shm example.
// See LICENSE.wawona and README.md for provenance.
#[allow(dead_code)]
#[path = "../../src/engine.rs"]
mod engine;
#[allow(clippy::too_many_arguments, dead_code)]
mod font;
#[allow(dead_code)]
#[path = "../../src/game.rs"]
mod game;
mod ui;

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn wawona_socket_close(fd: i32) -> i32;
    fn wawona_wayland_connect(fd_out: *mut i32) -> i32;
    fn wawona_wayland_shm_create(size: i32, fd_out: *mut i32) -> i32;
    fn wawona_wayland_shm_write(shm_fd: i32, offset: i32, buf: *const u8, len: i32) -> i32;
    fn wawona_wayland_sendmsg(wl_fd: i32, buf: *const u8, len: i32, scm_fd: i32) -> i32;
    fn wawona_socket_recv(fd: i32, buf: *mut u8, len: i32, n_out: *mut i32) -> i32;
}

const DISPLAY: u32 = 1;
const FORMAT_XRGB8888: u32 = 1;
const BTN_LEFT: u32 = 0x110;
const KEY_PRESSED: u32 = 1;
const DEFAULT_W: i32 = 640;
const DEFAULT_H: i32 = 800;
const MIN_W: i32 = 320;
const MIN_H: i32 = 480;
const MAX_W: i32 = 1920;
const MAX_H: i32 = 1080;

struct Conn {
    wl: i32,
    next_id: u32,
    registry: u32,
    compositor: u32,
    shm: u32,
    xdg_wm: u32,
    seat: u32,
    surface: u32,
    xdg_surface: u32,
    toplevel: u32,
    pointer: u32,
    keyboard: u32,
    touch: u32,
    pool: u32,
    buffer: u32,
    width: i32,
    height: i32,
    pending_w: i32,
    pending_h: i32,
    configured: bool,
    need_redraw: bool,
    closed: bool,
    ui: ui::Ui,
    ptr_x: i32,
    ptr_y: i32,
    ptr_inside: bool,
    shift: bool,
}

fn main() {
    let mut wl = 0i32;
    let rc = unsafe { wawona_wayland_connect(&mut wl) };
    if rc != 0 {
        eprintln!("wayland connect errno={rc} (is WAYLAND_DISPLAY set?)");
        std::process::exit(1);
    }

    let mut c = Conn {
        wl,
        next_id: 2,
        registry: 0,
        compositor: 0,
        shm: 0,
        xdg_wm: 0,
        seat: 0,
        surface: 0,
        xdg_surface: 0,
        toplevel: 0,
        pointer: 0,
        keyboard: 0,
        touch: 0,
        pool: 0,
        buffer: 0,
        width: DEFAULT_W,
        height: DEFAULT_H,
        pending_w: DEFAULT_W,
        pending_h: DEFAULT_H,
        configured: false,
        need_redraw: false,
        closed: false,
        ui: ui::Ui::default(),
        ptr_x: 0,
        ptr_y: 0,
        ptr_inside: false,
        shift: false,
    };

    c.registry = c.alloc();
    c.req(DISPLAY, 1, &c.registry.to_le_bytes());
    let cb = c.alloc();
    c.req(DISPLAY, 0, &cb.to_le_bytes());
    c.roundtrip_until_callback(cb);

    if c.compositor == 0 || c.shm == 0 || c.xdg_wm == 0 {
        eprintln!(
            "missing globals compositor={} shm={} xdg={} seat={}",
            c.compositor, c.shm, c.xdg_wm, c.seat
        );
        std::process::exit(1);
    }
    c.surface = c.alloc();
    c.req(c.compositor, 0, &c.surface.to_le_bytes());
    c.xdg_surface = c.alloc();
    let mut body = Vec::new();
    body.extend_from_slice(&c.xdg_surface.to_le_bytes());
    body.extend_from_slice(&c.surface.to_le_bytes());
    c.req(c.xdg_wm, 2, &body);
    c.toplevel = c.alloc();
    c.req(c.xdg_surface, 1, &c.toplevel.to_le_bytes());
    c.set_title(c.toplevel, "Chess for Wawona");
    c.set_min_size(c.toplevel, MIN_W, MIN_H);
    c.req(c.surface, 6, &[]);

    while !c.configured && !c.closed {
        if !c.recv_once() {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }
    if c.closed {
        std::process::exit(0);
    }

    c.apply_size();
    c.rebuild_buffer_and_paint();
    c.need_redraw = false;
    // Flush protocol validation after the first frame, using a guest-owned ID.
    let callback = c.alloc();
    c.req(DISPLAY, 0, &callback.to_le_bytes());
    println!(
        "chess-wawona: {}x{} XRGB8888 committed (xdg interactive)",
        c.width, c.height
    );
    eprintln!("chess-wawona: click / type / resize; close window to exit");

    while !c.closed {
        // One Wayland message at a time. With a blocking host fd this waits for
        // the next event; with nonblocking it returns false when empty. Paint
        // immediately after each event so configure / click / key never stall
        // behind a subsequent blocking recv.
        if c.recv_once() {
            if c.need_redraw && !c.closed {
                c.apply_size();
                c.rebuild_buffer_and_paint();
                c.need_redraw = false;
            }
        } else if !c.closed {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }
    unsafe {
        wawona_socket_close(c.wl);
    }
    eprintln!("chess-wawona: closed by compositor; exiting");
}

impl Conn {
    fn alloc(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn req(&self, obj: u32, opcode: u16, body: &[u8]) {
        let mut msg = Vec::new();
        put_header(&mut msg, obj, opcode, 8 + body.len() as u32);
        msg.extend_from_slice(body);
        self.sendmsg(&msg, -1);
    }

    fn sendmsg(&self, msg: &[u8], scm: i32) {
        let rc = unsafe { wawona_wayland_sendmsg(self.wl, msg.as_ptr(), msg.len() as i32, scm) };
        if rc != 0 {
            eprintln!("sendmsg errno={rc}");
            std::process::exit(1);
        }
    }

    fn set_title(&self, toplevel: u32, title: &str) {
        let mut body = Vec::new();
        put_string(&mut body, title);
        self.req(toplevel, 2, &body);
    }

    fn set_min_size(&self, toplevel: u32, w: i32, h: i32) {
        let mut body = Vec::new();
        body.extend_from_slice(&w.to_le_bytes());
        body.extend_from_slice(&h.to_le_bytes());
        self.req(toplevel, 8, &body);
    }

    fn apply_size(&mut self) {
        let mut w = if self.pending_w > 0 {
            self.pending_w
        } else {
            self.width.max(DEFAULT_W)
        };
        let mut h = if self.pending_h > 0 {
            self.pending_h
        } else {
            self.height.max(DEFAULT_H)
        };
        w = w.clamp(MIN_W, MAX_W);
        h = h.clamp(MIN_H, MAX_H);
        self.width = w;
        self.height = h;
    }

    /// Paint into a fresh SHM file, then create_pool (write-before-pool, same
    /// as weston-simple-shm style clients), attach, and commit.
    fn rebuild_buffer_and_paint(&mut self) {
        let stride = self.width * 4;
        let bytes = stride * self.height;
        let mut pixels = vec![0u8; bytes as usize];
        self.ui.paint(&mut pixels, self.width, self.height);

        let mut shm_fd = 0i32;
        let rc = unsafe { wawona_wayland_shm_create(bytes, &mut shm_fd) };
        if rc != 0 {
            eprintln!("shm_create errno={rc}");
            std::process::exit(1);
        }
        let rc =
            unsafe { wawona_wayland_shm_write(shm_fd, 0, pixels.as_ptr(), pixels.len() as i32) };
        if rc != 0 {
            eprintln!("shm_write errno={rc}");
            std::process::exit(1);
        }

        let old_pool = self.pool;
        let old_buffer = self.buffer;

        self.pool = self.alloc();
        let mut pool_msg = Vec::new();
        put_header(&mut pool_msg, self.shm, 0, 16);
        pool_msg.extend_from_slice(&self.pool.to_le_bytes());
        pool_msg.extend_from_slice(&bytes.to_le_bytes());
        self.sendmsg(&pool_msg, shm_fd);
        // The compositor owns the transferred fd; release the host handle.
        unsafe {
            wawona_socket_close(shm_fd);
        }

        self.buffer = self.alloc();
        let mut buf_body = Vec::new();
        buf_body.extend_from_slice(&self.buffer.to_le_bytes());
        buf_body.extend_from_slice(&0i32.to_le_bytes());
        buf_body.extend_from_slice(&self.width.to_le_bytes());
        buf_body.extend_from_slice(&self.height.to_le_bytes());
        buf_body.extend_from_slice(&stride.to_le_bytes());
        buf_body.extend_from_slice(&FORMAT_XRGB8888.to_le_bytes());
        self.req(self.pool, 0, &buf_body);

        // Attach + commit the new buffer before destroying the old one.
        let mut attach = Vec::new();
        attach.extend_from_slice(&self.buffer.to_le_bytes());
        attach.extend_from_slice(&0i32.to_le_bytes());
        attach.extend_from_slice(&0i32.to_le_bytes());
        self.req(self.surface, 1, &attach);
        // Content geometry for SSD hosts (matches buffer).
        // xdg_surface.set_window_geometry = opcode 3 (not 2; 2 is get_popup).
        let mut geom = Vec::new();
        geom.extend_from_slice(&0i32.to_le_bytes());
        geom.extend_from_slice(&0i32.to_le_bytes());
        geom.extend_from_slice(&self.width.to_le_bytes());
        geom.extend_from_slice(&self.height.to_le_bytes());
        self.req(self.xdg_surface, 3, &geom);
        let mut damage = Vec::new();
        damage.extend_from_slice(&0i32.to_le_bytes());
        damage.extend_from_slice(&0i32.to_le_bytes());
        damage.extend_from_slice(&self.width.to_le_bytes());
        damage.extend_from_slice(&self.height.to_le_bytes());
        self.req(self.surface, 2, &damage);
        self.req(self.surface, 6, &[]);

        if old_buffer != 0 {
            self.req(old_buffer, 0, &[]); // wl_buffer.destroy
        }
        if old_pool != 0 {
            self.req(old_pool, 1, &[]); // wl_shm_pool.destroy
        }
        eprintln!(
            "chess-wawona: committed {}x{} (interactive)",
            self.width, self.height
        );
    }

    fn roundtrip_until_callback(&mut self, cb: u32) {
        let mut done = false;
        let mut spins = 0u32;
        while !done {
            let (obj, opcode, payload) = match self.read_event() {
                Some(t) => t,
                None => {
                    // Nonblocking Wayland fd: wait for compositor bytes.
                    spins += 1;
                    if spins > 5000 {
                        eprintln!("chess-wawona: registry sync timed out");
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    continue;
                }
            };
            spins = 0;
            if obj == cb && opcode == 0 {
                done = true;
            } else if obj == self.registry && opcode == 0 {
                self.on_global(&payload);
            } else if obj == self.seat && opcode == 0 {
                self.on_seat(&payload);
            }
        }
    }

    fn recv_once(&mut self) -> bool {
        let (obj, opcode, payload) = match self.read_event() {
            Some(t) => t,
            None => return false,
        };
        if obj == DISPLAY && opcode == 0 {
            eprintln!("Wayland protocol error: {:?}", payload);
            std::process::exit(1);
        } else if obj == self.seat && opcode == 0 {
            self.on_seat(&payload);
        } else if obj == self.xdg_wm && opcode == 0 && payload.len() >= 4 {
            self.req(self.xdg_wm, 3, &payload[..4]);
        } else if obj == self.xdg_surface && opcode == 0 && payload.len() >= 4 {
            self.req(self.xdg_surface, 4, &payload[..4]);
            self.configured = true;
            self.need_redraw = true;
        } else if obj == self.toplevel && opcode == 0 && payload.len() >= 8 {
            let w = i32::from_le_bytes(payload[0..4].try_into().unwrap());
            let h = i32::from_le_bytes(payload[4..8].try_into().unwrap());
            if w > 0 {
                self.pending_w = w;
            }
            if h > 0 {
                self.pending_h = h;
            }
            eprintln!("chess-wawona: toplevel configure {}x{}", w, h);
        } else if obj == self.toplevel && opcode == 1 {
            self.closed = true;
        } else if self.pointer != 0 && obj == self.pointer {
            self.on_pointer(opcode, &payload);
        } else if self.keyboard != 0 && obj == self.keyboard {
            self.on_keyboard(opcode, &payload);
        } else if self.touch != 0 && obj == self.touch && opcode == 0 && payload.len() >= 24 {
            let x = fixed_to_i32(i32::from_le_bytes(payload[16..20].try_into().unwrap()));
            let y = fixed_to_i32(i32::from_le_bytes(payload[20..24].try_into().unwrap()));
            self.ui.click(x, y, self.width, self.height);
            self.need_redraw = true;
        }
        true
    }

    fn on_pointer(&mut self, opcode: u16, payload: &[u8]) {
        match opcode {
            // enter: serial, surface, x, y
            0 if payload.len() >= 16 => {
                self.ptr_inside = true;
                self.ptr_x = fixed_to_i32(i32::from_le_bytes(payload[8..12].try_into().unwrap()));
                self.ptr_y = fixed_to_i32(i32::from_le_bytes(payload[12..16].try_into().unwrap()));
            }
            1 => {
                self.ptr_inside = false;
            }
            // motion: time, x, y
            2 if payload.len() >= 12 => {
                self.ptr_x = fixed_to_i32(i32::from_le_bytes(payload[4..8].try_into().unwrap()));
                self.ptr_y = fixed_to_i32(i32::from_le_bytes(payload[8..12].try_into().unwrap()));
            }
            // button: time, serial, button, state
            3 if payload.len() >= 16 => {
                let button = u32::from_le_bytes(payload[8..12].try_into().unwrap());
                let state = u32::from_le_bytes(payload[12..16].try_into().unwrap());
                if button == BTN_LEFT && state == KEY_PRESSED && self.ptr_inside {
                    self.ui
                        .click(self.ptr_x, self.ptr_y, self.width, self.height);
                    self.need_redraw = true;
                }
            }
            _ => {}
        }
    }

    fn on_keyboard(&mut self, opcode: u16, payload: &[u8]) {
        match opcode {
            // modifiers
            4 if payload.len() >= 20 => {
                let depressed = u32::from_le_bytes(payload[4..8].try_into().unwrap());
                self.shift = depressed & 1 != 0;
            }
            // key: time, serial, key, state
            3 if payload.len() >= 16 => {
                let key = u32::from_le_bytes(payload[8..12].try_into().unwrap());
                let state = u32::from_le_bytes(payload[12..16].try_into().unwrap());
                if state != KEY_PRESSED {
                    return;
                }
                if key == 28 {
                    self.ui.submit();
                    self.need_redraw = true;
                    return;
                }
                if key == 1 {
                    self.ui.cancel();
                    self.need_redraw = true;
                    return;
                }
                if key == 14 {
                    // Backspace
                    self.ui.input.pop();
                    self.need_redraw = true;
                    return;
                }
                if let Some(ch) = keycode_to_char(key, self.shift)
                    && self.ui.input.len() < 256
                {
                    self.ui.input.push(ch);
                    self.need_redraw = true;
                }
            }
            _ => {}
        }
    }

    fn on_seat(&mut self, payload: &[u8]) {
        if payload.len() < 4 {
            return;
        }
        let capabilities = u32::from_le_bytes(payload[..4].try_into().unwrap());
        if capabilities & 1 != 0 && self.pointer == 0 {
            self.pointer = self.alloc();
            self.req(self.seat, 0, &self.pointer.to_le_bytes());
        }
        if capabilities & 2 != 0 && self.keyboard == 0 {
            self.keyboard = self.alloc();
            self.req(self.seat, 1, &self.keyboard.to_le_bytes());
        }
        if capabilities & 4 != 0 && self.touch == 0 {
            self.touch = self.alloc();
            self.req(self.seat, 2, &self.touch.to_le_bytes());
        }
    }

    fn on_global(&mut self, payload: &[u8]) {
        if payload.len() < 8 {
            return;
        }
        let name = u32::from_le_bytes(payload[0..4].try_into().unwrap());
        let (iface, rest) = take_string(&payload[4..]);
        if rest.len() < 4 {
            return;
        }
        let version = u32::from_le_bytes(rest[0..4].try_into().unwrap());
        let (want, ver) = match iface.as_str() {
            "wl_compositor" => {
                self.compositor = self.alloc();
                (self.compositor, version.min(4))
            }
            "wl_shm" => {
                self.shm = self.alloc();
                (self.shm, 1)
            }
            "xdg_wm_base" => {
                self.xdg_wm = self.alloc();
                (self.xdg_wm, version.min(2))
            }
            "wl_seat" => {
                self.seat = self.alloc();
                (self.seat, version.min(5))
            }
            _ => return,
        };
        let mut body = Vec::new();
        body.extend_from_slice(&name.to_le_bytes());
        put_string(&mut body, &iface);
        body.extend_from_slice(&ver.to_le_bytes());
        body.extend_from_slice(&want.to_le_bytes());
        self.req(self.registry, 0, &body);
    }

    fn read_event(&self) -> Option<(u32, u16, Vec<u8>)> {
        let mut hdr = [0u8; 8];
        if !recv_exact(self.wl, &mut hdr) {
            return None;
        }
        let obj = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        let size_op = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
        let size = (size_op >> 16) as usize;
        let opcode = (size_op & 0xffff) as u16;
        if size < 8 {
            return None;
        }
        let mut payload = vec![0u8; size - 8];
        if !payload.is_empty() && !recv_exact(self.wl, &mut payload) {
            return None;
        }
        Some((obj, opcode, payload))
    }
}

fn fixed_to_i32(f: i32) -> i32 {
    f / 256
}

fn keycode_to_char(key: u32, shift: bool) -> Option<char> {
    // Linux/evdev KEY_* as sent by Wayland wl_keyboard.key
    let ch = match key {
        2 => {
            if shift {
                '!'
            } else {
                '1'
            }
        }
        3 => {
            if shift {
                '@'
            } else {
                '2'
            }
        }
        4 => {
            if shift {
                '#'
            } else {
                '3'
            }
        }
        5 => {
            if shift {
                '$'
            } else {
                '4'
            }
        }
        6 => {
            if shift {
                '%'
            } else {
                '5'
            }
        }
        7 => {
            if shift {
                '^'
            } else {
                '6'
            }
        }
        8 => {
            if shift {
                '&'
            } else {
                '7'
            }
        }
        9 => {
            if shift {
                '*'
            } else {
                '8'
            }
        }
        10 => {
            if shift {
                '('
            } else {
                '9'
            }
        }
        11 => {
            if shift {
                ')'
            } else {
                '0'
            }
        }
        12 => {
            if shift {
                '_'
            } else {
                '-'
            }
        }
        13 => {
            if shift {
                '+'
            } else {
                '='
            }
        }
        16 => 'q',
        17 => 'w',
        18 => 'e',
        19 => 'r',
        20 => 't',
        21 => 'y',
        22 => 'u',
        23 => 'i',
        24 => 'o',
        25 => 'p',
        30 => 'a',
        31 => 's',
        32 => 'd',
        33 => 'f',
        34 => 'g',
        35 => 'h',
        36 => 'j',
        37 => 'k',
        38 => 'l',
        39 => {
            if shift {
                ':'
            } else {
                ';'
            }
        }
        40 => {
            if shift {
                '"'
            } else {
                '\''
            }
        }
        44 => 'z',
        45 => 'x',
        46 => 'c',
        47 => 'v',
        48 => 'b',
        49 => 'n',
        50 => 'm',
        51 => {
            if shift {
                '<'
            } else {
                ','
            }
        }
        52 => {
            if shift {
                '>'
            } else {
                '.'
            }
        }
        53 => {
            if shift {
                '?'
            } else {
                '/'
            }
        }
        57 => ' ',
        _ => return None,
    };
    Some(if shift && ch.is_ascii_lowercase() {
        ch.to_ascii_uppercase()
    } else {
        ch
    })
}

fn put_header(out: &mut Vec<u8>, obj: u32, opcode: u16, size: u32) {
    out.extend_from_slice(&obj.to_le_bytes());
    let size_op = (size << 16) | u32::from(opcode);
    out.extend_from_slice(&size_op.to_le_bytes());
}

fn put_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let n = bytes.len() + 1;
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out.push(0);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn take_string(data: &[u8]) -> (String, &[u8]) {
    if data.len() < 4 {
        return (String::new(), data);
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let padded = n.div_ceil(4) * 4;
    if data.len() < 4 + padded {
        return (String::new(), data);
    }
    let raw = &data[4..4 + n.saturating_sub(1)];
    let s = String::from_utf8_lossy(raw).into_owned();
    (s, &data[4 + padded..])
}

fn recv_exact(fd: i32, dest: &mut [u8]) -> bool {
    let mut off = 0;
    while off < dest.len() {
        let mut n = 0i32;
        let rc = unsafe {
            wawona_socket_recv(
                fd,
                dest[off..].as_mut_ptr(),
                (dest.len() - off) as i32,
                &mut n,
            )
        };
        if rc != 0 || n <= 0 {
            eprintln!("Wayland connection closed or receive failed (errno={rc})");
            std::process::exit(1);
        }
        off += n as usize;
    }
    true
}
