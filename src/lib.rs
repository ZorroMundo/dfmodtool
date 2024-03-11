#![feature(strict_provenance)]
#![feature(vec_into_raw_parts)]

use core::slice;
use std::{env, fs::File, io::{BufReader, BufWriter, Read, Write}, os::windows::process::CommandExt, process::Command};
use hudhook::{hooks::dx9::ImguiDx9Hooks, *};
use mmap_rs::MemoryAreas;
use rand::Rng;
use rfd::FileDialog;

hudhook!(ImguiDx9Hooks, RenderLoop::default());

pub struct RenderLoop {
    end_setup: bool,
    is_hidding: bool,
    is_w1_transitioning: u16,
    music: ListBoxData,
    music_entry: Vec<MusicEntry>,
    w1_position: ([f32; 2], imgui::Condition),
    last_w1_position: [f32; 2],
}

#[derive(Default)]
pub struct ListBoxData {
    item: i32,
    items: Vec<String>,
}

#[derive(Default)]
pub struct MusicEntry {
    size: u32,
    offset: usize,
    entry: usize, // *mut u32
    entry_ptr: usize, // *mut u8
    local_ptr: usize, // *mut u8
    local_ptr2: usize, // *mut u8,
    local_size_ptr: usize, // *mut u32
    new_music: Option<(usize, usize, usize)>, // *mut u8, size, capacity
}

impl Default for RenderLoop {
    fn default() -> Self {
        Self {
            end_setup: false,
            is_hidding: false,
            is_w1_transitioning: 0,
            music: ListBoxData::default(),
            music_entry: Vec::new(),
            w1_position: (
                [15., 15.],
                imgui::Condition::Never
            ),
            last_w1_position: [15., 15.],
        }
    }
}

impl RenderLoop {
    unsafe fn refresh_music_data(&mut self) {
        // DF 2.7.8b stuff
        // 0x01b1fbba: Texture ptr
        // 0x026d1562: Music ptr
        // 0x01a395e6: String ptr
        println!("========== Started localizing pointers ==========");
        const MUSIC_SIZE_POINTER: usize = 0x026d1562 + 8;
        const MUSIC_FIRST_ENTRY_POINTER: usize = 0x026d1562 + 12;
        let size = *(MUSIC_SIZE_POINTER as *mut u32);
        let mut offset = 0;
        let mut pointers = Vec::new();
        for i in 0..size {
            if i == 0 {
                offset = (MUSIC_FIRST_ENTRY_POINTER - *(MUSIC_FIRST_ENTRY_POINTER as *mut u32) as usize) + (size as usize * 4);
            }
            let ptr = MUSIC_FIRST_ENTRY_POINTER + (i as usize * 4);
            pointers.push(((*(ptr as *mut u32)) as usize + offset, ptr));
            self.music.items.push(format!("EmbeddedSound {i}"));
        }
        for (ptr, sptr) in pointers {
            let audio_size = *(ptr as *mut u32);
            self.music_entry.push(MusicEntry {
                offset,
                entry: ptr,
                size: audio_size,
                entry_ptr: sptr,
                ..Default::default()
            });
        }
        const STRING_FIRST_ENTRY_POINTER: usize = 0x01a395e6 + 12;
        let mut size = 0;
        while *((STRING_FIRST_ENTRY_POINTER + (size as usize * 4)) as *mut u32) > 0x80 {
            size += 1;
        }
        let mut offset = 0;
        let mut pointers = Vec::new();
        for i in 0..size {
            if i == 0 {
                offset = (STRING_FIRST_ENTRY_POINTER - *(STRING_FIRST_ENTRY_POINTER as *mut u32) as usize) + (size as usize * 4);
            }
            let ptr = STRING_FIRST_ENTRY_POINTER + (i as usize * 4);
            pointers.push(((*(ptr as *mut u32)) as usize + offset, ptr));
        }
        /*for (ptr, sptr) in pointers {
            //println!("{ptr}");
            self.string_entry.push(StringEntry {
                offset,
                entry: ptr,
                entry_ptr: sptr,
                string: CString::from_raw((ptr + 4) as *mut i8)
            });
        }*/
        // External pointer data
        let mut current_audiogroup = 0;
        for ma in MemoryAreas::open(None).unwrap().flatten() {
            if ma.end() - ma.start() > 0xffff {
                let ptr = (ma.start() + 0x30) as *mut u8;
                if &String::from_utf8_lossy(slice::from_raw_parts(ptr, 4)) == "FORM" {
                    // Found the external audiogroup pointer
                    let size = *(ptr.offset(0x10) as *mut u32);
                    let first_entry_pointer = ptr.offset(0x14) as *mut u32;
                    let mut pointers = Vec::new();
                    offset = 0;
                    for i in 0..size {
                        if i == 0 {
                            offset = (first_entry_pointer.addr() - *first_entry_pointer as usize) + (size as usize * 4);
                        }
                        let ptr = first_entry_pointer.addr() + (i as usize * 4);
                        pointers.push((*(ptr as *mut u32) as usize + offset, ptr));
                        self.music.items.push(format!("AudioGroup {current_audiogroup} EmbeddedSound {i}"));
                    }
                    for (eptr, sptr) in pointers {
                        let audio_size = *(eptr as *mut u32);
                        self.music_entry.push(MusicEntry {
                            offset,
                            entry: eptr,
                            size: audio_size,
                            entry_ptr: sptr,
                            ..Default::default()
                        });
                    }
                    current_audiogroup += 1;
                }
            }
        }
        for ma in MemoryAreas::open(None).unwrap().flatten() {
            if ma.end() - ma.start() > (1024 * 1024) && ma.start() < 0x10000000 && ma.end() - ma.start() < (1024 * 1024 * 4) {
                // We have no way to know how the heap will end up allocating the pointers
                println!("New Memory Area at 0x{:x}", ma.start());
                let mut offset = ma.start();
                while offset < ma.end() {
                    for me in &mut self.music_entry {
                        if me.entry == *(offset as *mut u32) as usize {
                            println!("Found a pointer at 0x{:x} for 0x{:x}", offset, me.entry);
                            me.local_ptr = offset;
                        }
                        if me.entry + 4 == *(offset as *mut u32) as usize {
                            println!("Found a pointer at 0x{:x} for 0x{:x} + 0x04", offset, me.entry);
                            me.local_ptr2 = offset;
                            me.local_size_ptr = offset + 4;
                        }
                    }
                    offset += 0x04;
                }
            }
        }
        println!("========== Finished localizing pointers ==========");
    }
}

impl ImguiRenderLoop for RenderLoop {
    fn render(&mut self, ui: &mut imgui::Ui) {
        if !self.end_setup {
            self.end_setup = true;
            unsafe {
                windows::Win32::System::Console::AllocConsole().unwrap();
                self.refresh_music_data();
            }
        }
        if self.is_w1_transitioning > 0 {
            self.is_w1_transitioning -= 1;
        }
        if ui.is_key_pressed(imgui::Key::Tab) {
            self.is_hidding = !self.is_hidding;
            if !self.is_hidding {
                self.is_w1_transitioning = 30;
            }
        }
        ui.window("DF Mod Tool")
            .position([15., 15.], imgui::Condition::FirstUseEver)
            .position(self.w1_position.0, self.w1_position.1)
            .size([420., 420.], imgui::Condition::FirstUseEver)
            .build(|| {
                const T: f32 = 0.4;
                if self.is_hidding {
                    self.w1_position.0[0] = self.w1_position.0[0] * (1. - T) + (-(ui.window_size()[0] + 4.) * T);
                    //self.w1_position.0[1] = self.w1_position.0[1] * (1. - T) + (-ui.window_size()[1] * T);
                    self.w1_position.1 = imgui::Condition::Always;
                } else if self.is_w1_transitioning > 0 {
                    //self.w1_position.0 = self.last_w1_position;
                    self.w1_position.0[0] = self.w1_position.0[0] * (1. - T) + (self.last_w1_position[0] * T);
                    self.w1_position.0[1] = self.w1_position.0[1] * (1. - T) + (self.last_w1_position[1] * T);
                } else if self.w1_position.1 == imgui::Condition::Always {
                    self.w1_position.1 = imgui::Condition::Never;
                } else {
                    self.last_w1_position = ui.window_pos();
                    self.last_w1_position[0] = self.last_w1_position[0].max(15.);
                    self.last_w1_position[1] = self.last_w1_position[1].max(15.);
                }
                ui.text("DF Mod Tool by ZorroMundo");
                ui.text("Current Version: v1.0.0a1");
                ui.text("Hide the Window using the TAB key");
                ui.separator();
                ui.text_colored([0.2, 1., 0.2, 1.], "General Functions");
                let mut game_id = unsafe { (*(0xa4e5e0 as *mut i32)).to_string() };
                ui.input_text("Game ID", &mut game_id).build();
                unsafe {
                    if let Ok(n) = game_id.parse::<i32>() {
                        if *(0xa4e5e0 as *mut i32) != n {
                            println!("========== Changed Game ID to {n} ==========");
                            *(0xa4e5e0 as *mut i32) = n;
                        }
                    }
                }
                // This seems to have no effect on the game.
                /*let mut owner_id = unsafe { (*(0x1794646 as *mut i32)).to_string() };
                ui.input_text("Owner ID", &mut owner_id).build();
                unsafe {
                    if let Ok(n) = owner_id.parse::<i32>() {
                        if *(0x1794646 as *mut i32) != n {
                            println!("========== Changed Owner ID to {n} ==========");
                            *(0x1794646 as *mut i32) = n;
                            *(0x179497a as *mut i32) = n;
                            *(0x186c582 as *mut i32) = n;
                        }
                    }
                }*/
                // The owner name should go here.
                // But, GM is modifying the memory
                // that is allocated here in Rust
                // We'll need to investigate more
                // on how to peform it correctly.
                ui.separator();
                ui.text_colored([1., 0.5, 0., 1.], "Music Functions");
                if ui.button("Save") {
                    let data = unsafe {
                        if let Some((ptr, size, _capacity)) = self.music_entry[self.music.item as usize].new_music {
                            slice::from_raw_parts(
                                (ptr + 4) as *mut u8,
                                size)
                        } else {
                            slice::from_raw_parts(
                                (self.music_entry[self.music.item as usize].entry + 4) as *mut u8,
                                self.music_entry[self.music.item as usize].size as usize)
                        }
                    };
                    let is_ogg = &data[0..4] == b"OggS";
                    let file = FileDialog::new()
                        .add_filter(if is_ogg { "OGG Files" } else { "WAV Files" },
                         &[if is_ogg { "ogg" } else { "wav" }])
                        .set_file_name(self.music.items[self.music.item as usize].clone())
                        .save_file();
                    if let Some(file) = file {
                        let mut f = BufWriter::new(File::create(file).unwrap());
                        f.write_all(data).unwrap();
                        f.flush().unwrap();
                        drop(f);
                    }
                }
                ui.same_line();
                if ui.button("Save & Play") {
                    let data = unsafe {
                        if let Some((ptr, size, _capacity)) = self.music_entry[self.music.item as usize].new_music {
                            slice::from_raw_parts(
                                (ptr + 4) as *mut u8,
                                size)
                        } else {
                            slice::from_raw_parts(
                                (self.music_entry[self.music.item as usize].entry + 4) as *mut u8,
                                self.music_entry[self.music.item as usize].size as usize)
                        }
                    };
                    let is_ogg = &data[0..4] == b"OggS";
                    let file = FileDialog::new()
                        .add_filter(if is_ogg { "OGG Files" } else { "WAV Files" },
                            &[if is_ogg { "ogg" } else { "wav" }])
                        .set_file_name(self.music.items[self.music.item as usize].clone())
                        .save_file();
                    if let Some(file) = file {
                        let mut f = BufWriter::new(File::create(&file).unwrap());
                        f.write_all(data).unwrap();
                        f.flush().unwrap();
                        drop(f);
                        Command::new("cmd").creation_flags(0x08000000)
                            .arg("/c").arg("start").arg("")
                            .arg(file)
                            .spawn().unwrap();
                    }
                }
                ui.same_line();
                if ui.button("Temp Save & Play") {
                    let data = unsafe {
                        if let Some((ptr, size, _capacity)) = self.music_entry[self.music.item as usize].new_music {
                            slice::from_raw_parts(
                                (ptr + 4) as *mut u8,
                                size)
                        } else {
                            slice::from_raw_parts(
                                (self.music_entry[self.music.item as usize].entry + 4) as *mut u8,
                                self.music_entry[self.music.item as usize].size as usize)
                        }
                    };
                    let mut file = env::temp_dir();
                    let is_ogg = &data[0..4] == b"OggS";
                    file.push(rand::thread_rng().gen_range(0..0xffffff).to_string() + if is_ogg { ".ogg" } else { ".wav" });
                    let mut f = BufWriter::new(File::create(&file).unwrap());
                    f.write_all(data).unwrap();
                    f.flush().unwrap();
                    drop(f);
                    Command::new("cmd").creation_flags(0x08000000)
                        .arg("/c").arg("start").arg("")
                        .arg(file)
                        .spawn().unwrap();
                }
                if ui.button("Load") {
                    let data = unsafe {
                        if let Some((ptr, size, _capacity)) = self.music_entry[self.music.item as usize].new_music {
                            slice::from_raw_parts(
                                (ptr + 4) as *mut u8,
                                size)
                        } else {
                            slice::from_raw_parts(
                                (self.music_entry[self.music.item as usize].entry + 4) as *mut u8,
                                self.music_entry[self.music.item as usize].size as usize)
                        }
                    };
                    let is_ogg = &data[0..4] == b"OggS";
                    let file = FileDialog::new()
                        .add_filter(if is_ogg { "OGG Files" } else { "WAV Files" },
                        &[if is_ogg { "ogg" } else { "wav" }])
                        .set_file_name(self.music.items[self.music.item as usize].clone())
                        .pick_file();
                    if let Some(file) = file {
                        let mut f = BufReader::new(File::open(file).unwrap());
                        let mut data = Vec::new();
                        f.read_to_end(&mut data).unwrap();
                        drop(f);
                        let mut final_data = Vec::new();
                        final_data.extend((data.len() as u32).to_le_bytes());
                        final_data.extend(data);
                        let final_data = final_data.into_raw_parts();
                        unsafe {
                            let entry = &mut self.music_entry[self.music.item as usize];
                            println!("========== Loaded new song ==========");
                            println!("Main entry pointer: {:?}", entry.entry_ptr);
                            println!("New data pointer: {:?}", final_data.0.addr());
                            println!("Entry pointer: {:?}", entry.entry);
                            println!("Entry size: {:?}", entry.size);
                            println!("Local pointer: {:?}", entry.local_ptr);
                            println!("Second Local pointer: {:?}", entry.local_ptr2);
                            *(entry.entry_ptr as *mut u32) = (final_data.0.addr() - entry.offset) as u32;
                            *(entry.local_ptr as *mut u32) = final_data.0.addr() as u32;
                            if entry.local_ptr2 != 0 {
                                *(entry.local_ptr2 as *mut u32) = (final_data.0.addr() + 4) as u32;
                                *(entry.local_size_ptr as *mut u32) = (final_data.1 - 4) as u32;
                            } else {
                                println!("========== Invalid pointer for replacing audio ==========");
                            }
                            entry.new_music = Some((final_data.0.addr(), final_data.1, final_data.2));
                        }
                    }
                }
                ui.same_line();
                if ui.button("Restore OG Song") {
                    unsafe {
                        let entry = &mut self.music_entry[self.music.item as usize];
                        if let Some((ptr, size, capacity)) = entry.new_music.take() {
                            println!("========== Restored old song ==========");
                            println!("Main entry pointer: {:?}", entry.entry_ptr);
                            println!("Entry pointer: {:?}", entry.entry);
                            println!("Entry size: {:?}", entry.size);
                            println!("Local pointer: {:?}", entry.local_ptr);
                            println!("Second Local pointer: {:?}", entry.local_ptr2);
                            *(entry.entry_ptr as *mut u32) = (entry.entry - entry.offset) as u32;
                            *(entry.local_ptr as *mut u32) = entry.entry as u32;
                            if entry.local_ptr2 != 0 {
                                *(entry.local_ptr2 as *mut u32) = (entry.entry + 4) as u32;
                                *(entry.local_size_ptr as *mut u32) = entry.size;
                            }
                            // Deallocate the song
                            drop(Vec::from_raw_parts(ptr as *mut u8, size, capacity));
                        } else {
                            println!("========== The song has not been modified ==========");
                        }
                    }
                }
                ui.list_box("Music Data", &mut self.music.item, &self.music.items.iter().collect::<Vec<&String>>(), 10);
            });
    }
}