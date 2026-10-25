//! DOSCALLS: the kernel API.

use alloc::string::String;

use super::{err, Ctx, Handler};
use super::file;
use crate::loader;
use crate::task::{process, sched};
use crate::vm::{self, pag, SegKind};

/// Implementation for a gate entry, if there is one.
pub fn handler(module: u8, ordinal: u16) -> Option<Handler> {
    use super::{kbdapi as k, msg, nls, vio as v};
    let h: Handler = match (module, ordinal) {
        // ---- VIOCALLS ----
        (1, 1) => v::vio_end_pop_up,
        (1, 2) => v::vio_get_phys_buf,
        (1, 3) => v::vio_get_ansi,
        (1, 5) => v::vio_set_ansi,
        (1, 6) => super::subsys::vio_deregister,
        (1, 7) => v::vio_scroll_up,
        (1, 8) => v::vio_prt_sc,
        (1, 9) => v::vio_get_cur_pos,
        (1, 10) => v::vio_wrt_cell_str,
        (1, 11) => v::vio_pop_up,
        (1, 12) => v::vio_scroll_rt,
        (1, 13) => v::vio_wrt_char_str,
        (1, 15) => v::vio_set_cur_pos,
        (1, 18) => v::vio_scr_unlock,
        (1, 19) => v::vio_wrt_tty,
        (1, 21) => v::vio_get_mode,
        (1, 22) => v::vio_set_mode,
        (1, 23) => v::vio_scr_lock,
        (1, 24) => v::vio_read_cell_str,
        (1, 25) => v::vio_sav_redraw_wait,
        (1, 26) => v::vio_wrt_n_attr,
        (1, 27) => v::vio_get_cur_type,
        (1, 28) => v::vio_sav_redraw_undo,
        (1, 29) => v::vio_get_font,
        (1, 30) => v::vio_read_char_str,
        (1, 31) => v::vio_get_buf,
        (1, 32) => v::vio_set_cur_type,
        (1, 33) => v::vio_set_font,
        (1, 35) => v::vio_mode_undo,
        (1, 37) => v::vio_mode_wait,
        (1, 40) => v::vio_get_cp,
        (1, 42) => v::vio_set_cp,
        (1, 43) => v::vio_show_buf,
        (1, 44) => v::vio_scroll_lf,
        (1, 45) => super::subsys::vio_register,
        (1, 46) => v::vio_get_config,
        (1, 47) => v::vio_scroll_dn,
        (1, 48) => v::vio_wrt_char_str_att,
        (1, 49) => v::vio_get_state,
        (1, 50) => v::vio_prt_sc_toggle,
        (1, 51) => v::vio_set_state,
        (1, 52) => v::vio_wrt_n_cell,
        (1, 53) => v::vio_wrt_n_char,
        (1, 55) => super::avio::associate,
        (1, 56) => super::avio::create_ps,
        (1, 57) => super::avio::delete_set_id,
        (1, 58) => super::avio::get_cell_size,
        (1, 59) => super::avio::get_org,
        (1, 60) => super::avio::create_log_font,
        (1, 61) => super::avio::destroy_ps,
        (1, 62) => super::avio::query_set_ids,
        (1, 63) => super::avio::set_org,
        (1, 64) => super::avio::query_fonts,
        (1, 65) => super::avio::set_cell_size,
        (1, 66) => super::avio::show_ps,
        (1, 70) => super::subsys::vio_global_reg,
        (1, 75) => v::vio_check_char_type,
        // ---- KBDCALLS ----
        (2, 1) => k::kbd_set_cust_xt,
        (2, 3) => nls::kbd_get_cp,
        (2, 4) => k::kbd_char_in,
        (2, 5) => nls::kbd_set_cp,
        (2, 7) => k::ok,           // KbdSynch
        (2, 8) => super::subsys::kbd_register,
        (2, 9) => k::kbd_string_in,
        (2, 10) => k::kbd_get_status,
        (2, 11) => k::kbd_set_status,
        (2, 12) => k::kbd_get_focus,
        (2, 13) => k::kbd_flush_buffer,
        (2, 14) => k::kbd_xlate,
        (2, 17) => k::kbd_close,
        (2, 18) => k::kbd_free_focus,
        (2, 20) => super::subsys::kbd_deregister,
        (2, 21) => k::ok,          // KbdSetFgnd
        (2, 22) => k::kbd_peek,
        (2, 23) => k::kbd_open,
        (2, 24) => k::kbd_get_hwid,
        (2, 25) => k::ok,          // KbdSetHWID
        // ---- MONCALLS ----
        (9, 1) => super::moncalls::dos_mon_write,
        (9, 2) => super::moncalls::dos_mon_read,
        (9, 3) => super::moncalls::dos_mon_close,
        (9, 4) => super::moncalls::dos_mon_open,
        (9, 5) => super::moncalls::dos_mon_reg,
        // ---- MSG ----
        (4, 1) | (4, 5) => msg::dos_put_message,
        (4, 2) | (4, 6) => msg::dos_true_get_message,
        (4, 3) | (4, 4) => msg::dos_ins_message,
        (4, 7) | (4, 8) => msg::dos_query_message_cp,
        // ---- NLS ----
        (5, 1) | (5, 7) => nls::dos_case_map,
        (5, 2) | (5, 8) => nls::dos_get_collate,
        (5, 3) | (5, 5) => nls::dos_get_ctry_info,
        (5, 4) | (5, 6) => nls::dos_get_dbcs_env,
        // ---- QUECALLS (16-bit 1-8, 32-bit 9-16) ----
        (7, 1) | (7, 9) => super::queue::dos_read_queue,
        (7, 2) | (7, 10) => super::queue::dos_purge_queue,
        (7, 3) | (7, 11) => super::queue::dos_close_queue,
        (7, 4) | (7, 12) => super::queue::dos_query_queue,
        (7, 5) | (7, 13) => super::queue::dos_peek_queue,
        (7, 6) | (7, 14) => super::queue::dos_write_queue,
        (7, 7) | (7, 15) => super::queue::dos_open_queue,
        (7, 8) | (7, 16) => super::queue::dos_create_queue,
        // ---- More DOSCALLS ----
        (0, 13) => ok,  // DosHoldSignal
        (0, 14) => dos16_set_sig_handler,
        (0, 43) => dos16_create_cs_alias,
        (0, 52) | (0, 231) => dos_dev_config,
        (0, 130) => nls::dos16_get_cp,
        (0, 153) | (0, 288) => nls::dos_set_cp,
        (0, 164) | (0, 289) => nls::dos_set_process_cp,
        (0, 567) => file::dos16_open_l,
        (0, 568) => file::dos16_set_file_ptr_l,
        (0, 570) => file::dos16_set_file_size_l,
        (0, 2) | (0, 280) => super::proc::dos_wait_child,
        (0, 3) | (0, 232) => super::proc::dos_enter_crit_sec,
        (0, 6) | (0, 233) => super::proc::dos_exit_crit_sec,
        (0, 9) => super::proc::dos16_get_prty,
        (0, 10) | (0, 235) => super::proc::dos_kill_process,
        (0, 11) | (0, 236) => super::proc::dos_set_priority,
        (0, 26) | (0, 237) => super::proc::dos_resume_thread,
        (0, 27) | (0, 238) => super::proc::dos_suspend_thread,
        (0, 111) => super::proc::dos_kill_thread,
        (0, 144) | (0, 283) => super::proc::dos_exec_pgm,
        (0, 145) => super::proc::dos16_create_thread,
        (0, 156) => super::proc::dos16_get_ppid,
        (0, 311) => super::proc::dos_create_thread,
        (0, 349) => super::proc::dos_wait_thread,
        (0, 20) => super::sem::dos16_sem_set_wait,
        (0, 22) => super::sem::dos16_mux_sem_wait,
        (0, 23) => super::sem::dos16_close_sem,
        (0, 24) => super::sem::dos16_create_sem,
        (0, 25) => super::sem::dos16_open_sem,
        (0, 140) => super::sem::dos16_sem_request,
        (0, 141) => super::sem::dos16_sem_clear,
        (0, 142) => super::sem::dos16_sem_wait,
        (0, 143) => super::sem::dos16_sem_set,
        (0, 324) => super::sem::dos_create_event_sem,
        (0, 325) => super::sem::dos_open_event_sem,
        (0, 326) => super::sem::dos_close_event_sem,
        (0, 327) => super::sem::dos_reset_event_sem,
        (0, 328) => super::sem::dos_post_event_sem,
        (0, 329) => super::sem::dos_wait_event_sem,
        (0, 330) => super::sem::dos_query_event_sem,
        (0, 331) => super::sem::dos_create_mutex_sem,
        (0, 332) => super::sem::dos_open_mutex_sem,
        (0, 333) => super::sem::dos_close_mutex_sem,
        (0, 334) => super::sem::dos_request_mutex_sem,
        (0, 335) => super::sem::dos_release_mutex_sem,
        (0, 336) => super::sem::dos_query_mutex_sem,
        (0, 337) => super::sem::dos_create_mux_wait_sem,
        (0, 338) => super::sem::dos_open_mux_wait_sem,
        (0, 339) => super::sem::dos_close_mux_wait_sem,
        (0, 340) => super::sem::dos_wait_mux_wait_sem,
        (0, 341) => super::sem::dos_add_mux_wait_sem,
        (0, 342) => super::sem::dos_delete_mux_wait_sem,
        (0, 343) => super::sem::dos_query_mux_wait_sem,
        (0, 7) | (0, 296) => super::proc::dos_exit_list,
        (0, 356) => super::exceptions::dos_raise_exception,
        (0, 357) => super::exceptions::dos_unwind_exception,
        (0, 378) => super::xproc::dos_set_signal_exception_focus,
        (0, 379) => super::xproc::dos_send_signal_exception,
        (0, 380) => super::xproc::dos_enter_must_complete,
        (0, 381) => super::xproc::dos_exit_must_complete,
        (0, 418) => super::xproc::dos_acknowledge_signal_exception,
        (0, 134) => super::xproc::dos16_send_signal,
        (0, 15) => super::xproc::dos16_flag_process,
        (0, 454) => super::xproc::dos_alloc_thread_local_memory,
        (0, 455) => super::xproc::dos_free_thread_local_memory,
        (0, 997) => super::xproc::dos_create_thread2,
        (0, 460) => super::xproc::dos_verify_pid_tid,
        (0, 360) => super::xproc::dos_query_mod_from_eip,
        (0, 113) => super::xproc::dos_dump_process,
        (0, 114) => super::xproc::dos_suppress_popups,
        (0, 444) => not_supported, // DosForceSystemDump
        (0, 161) => super::xproc::dos16_fsram_sem_request,
        (0, 162) => super::xproc::dos16_fsram_sem_clear,
        (0, 90) => super::xproc::dos16_sys_trace,
        (0, 877) => super::xproc::dos_query_thread_context,
        // ---- System services ----
        (0, 876) => super::sysctl::dos_sys_ctl,
        (0, 995) => super::sysctl::dos_system_service,
        (0, 112) => super::sysctl::dos_query_ras_info,
        (0, 371) => super::sysctl::dos_query_abios_support,
        (0, 976) => super::sysctl::dos_perf_sys_call,
        (0, 364) => super::perf::dos_alias_perf_ctrs,
        (0, 365) => super::perf::dos_configure_perf,
        (0, 139) | (0, 211) => super::sysctl::dos_err_class,
        (0, 166) => super::sysctl::dos16_qsys_info,
        (0, 447) => super::sysctl::dos_get_processor_status,
        (0, 448) => super::sysctl::dos_set_processor_status,
        (0, 453) => super::sysctl::dos_test_psd,
        (0, 557) => super::sysctl::dos_create_spin_lock,
        (0, 558) => super::sysctl::dos_acquire_spin_lock,
        (0, 559) => super::sysctl::dos_release_spin_lock,
        (0, 560) => super::sysctl::dos_free_spin_lock,
        // ---- File system control, locks, protected handles ----
        (0, 285) => super::fsctl::dos_fsctl,
        (0, 183) => super::fsctl::dos16_fsctl,
        (0, 181) | (0, 269) => super::fsctl::dos_fs_attach,
        (0, 428) => super::filelock::dos_set_file_locks,
        (0, 986) => super::filelock::dos_set_file_locks_l,
        (0, 62) => super::filelock::dos16_file_locks,
        (0, 429) | (0, 987) => super::filelock::dos_cancel_lock_request,
        (0, 637) => super::filelock::dos_protect_open,
        (0, 982) => super::filelock::dos_protect_open_l,
        (0, 639) => super::filelock::dos_protect_set_file_locks,
        (0, 983) => super::filelock::dos_protect_set_file_locks_l,
        (0, 638) => file::dos_close,
        (0, 641) => file::dos_read,
        (0, 642) => file::dos_write,
        (0, 621) => file::dos_set_file_ptr,
        (0, 640) => file::dos_set_file_size,
        (0, 643) => file::dos_set_file_info,
        (0, 644) => file::dos_set_fh_state,
        (0, 645) => file::dos_query_fh_state,
        (0, 646) => file::dos_query_file_info,
        (0, 636) => file::dos_enum_attribute,
        (0, 984) => file::dos_set_file_ptr_l,
        (0, 985) => file::dos_set_file_size_l,
        // ---- Memory ----
        (0, 298) => super::memx::dos_alias_mem,
        (0, 307) => super::memx::dos_query_mem_state,
        (0, 42) => super::memx::dos16_realloc_huge,
        (0, 122) | (0, 123) => super::memx::dos16_lock_seg,
        // ---- PM variants of the semaphore calls (no message queue here) ----
        (0, 561) | (0, 591) => super::sem::dos_wait_event_sem,
        (0, 562) | (0, 590) => super::sem::dos_post_event_sem,
        (0, 593) => super::sem::dos_request_mutex_sem,
        (0, 595) => super::sem::dos_wait_mux_wait_sem,
        (0, 16) => file::dos16_make_pipe,
        (0, 239) => file::dos_create_pipe,
        // ---- RS2API ----
        (8, 1) => super::rs2api::rs2_map_display,
        (8, 2) => super::rs2api::rs2_read_input,
        (8, 3) => super::rs2api::rs2_query_pointer,
        (8, 4) => super::rs2api::rs2_debug_trace,
        (8, 5) => super::rs2api::rs2_set_pointer_shape,
        (8, 500) => super::avio::query,
        (8, 501) => super::avio::font,
        (8, 6) => super::rs2api::rs2_show_pointer,
        (8, 7) => super::rs2api::rs2_power_off,
        (8, 20) => super::rs2api::rs2_update_display,
        (8, 21) => super::rs2api::rs2_query_display,
        (8, 22) => super::rs2api::rs2_query_display_modes,
        (8, 23) => super::rs2api::rs2_set_display_mode,
        (8, 24) => super::rs2api::rs2_query_processes,
        (8, 25) => super::rs2api::rs2_query_thread_report,
        (8, 26) => super::rs2api::rs2_kill_process,
        (8, 10) => super::viowin::wait,
        (8, 11) => super::viowin::query_sessions,
        (8, 12) => super::viowin::read_screen,
        (8, 13) => super::viowin::query_font,
        (8, 14) => super::viowin::key,
        (8, 15) => super::viowin::mouse,
        (8, 16) => super::viowin::control,
        (8, 17) => super::viowin::read_history,
        (8, 400) => super::vdm::register,
        (8, 401) => super::vdm::read_input,
        (8, 402) => super::vdm::set_text,
        (8, 403) => super::vdm::set_pixels,
        (8, 404) => super::vdm::read_pixels,
        (8, 405) => super::vdm::mouse,
        (8, 406) => super::vdm::surface,
        (8, 420) => super::win16::set_int_handler,
        (8, 407) => super::vdm::request,
        (8, 100) => super::sysctl::rs2_query_counters,
        (8, 200) => super::sockets::query_iface,
        (8, 201) => super::sockets::create,
        (8, 202) => super::sockets::bind,
        (8, 203) => super::sockets::listen,
        (8, 204) => super::sockets::accept,
        (8, 205) => super::sockets::connect,
        (8, 206) => super::sockets::send,
        (8, 207) => super::sockets::recv,
        (8, 208) => super::sockets::send_to,
        (8, 209) => super::sockets::recv_from,
        (8, 210) => super::sockets::shutdown,
        (8, 211) => super::sockets::close,
        (8, 212) => super::sockets::select,
        (8, 213) => super::sockets::ioctl,
        (8, 214) => super::sockets::set_opt,
        (8, 215) => super::sockets::get_opt,
        (8, 216) => super::sockets::get_name,
        (8, 217) => super::sockets::resolve,
        (8, 218) => super::sockets::cancel,
        (8, 219) => super::sockets::query_sockets,
        (8, 220) => super::sockets::take,
        (8, 221) => super::sockets::release,
        (8, 300) => super::audio::query,
        (8, 301) => super::audio::open,
        (8, 302) => super::audio::write,
        (8, 303) => super::audio::query_pos,
        (8, 304) => super::audio::drain,
        (8, 305) => super::audio::stop,
        (8, 306) => super::audio::close,
        (8, 307) => super::audio::set_volume,
        (8, 308) => super::audio::pause,
        (8, 309) => super::audio::open_input,
        (8, 310) => super::audio::read,
        (8, 311) => super::audio::query_level,
        (8, 312) => super::audio::set_input,
        // ---- MOUCALLS ----
        (3, 1) => super::mou::mou_get_ptr_shape,
        (3, 2) => super::mou::mou_set_ptr_shape,
        (3, 3) => super::mou::mou_get_num_mickeys,
        (3, 6) => super::mou::mou_get_scale_fact,
        (3, 7) => super::mou::mou_flush_que,
        (3, 8) => super::mou::mou_get_num_buttons,
        (3, 9) => super::mou::mou_close,
        (3, 11) => super::mou::mou_set_scale_fact,
        (3, 13) => super::mou::mou_get_num_que_el,
        (3, 14) => super::subsys::mou_deregister,
        (3, 24) => super::subsys::mou_register,
        (3, 23) | (3, 27) | (3, 30) => super::mou::ok,
        (3, 15) => super::mou::mou_get_event_mask,
        (3, 16) => super::mou::mou_set_event_mask,
        (3, 17) => super::mou::mou_open,
        (3, 18) => super::mou::mou_remove_ptr,
        (3, 19) => super::mou::mou_get_ptr_pos,
        (3, 20) => super::mou::mou_read_event_que,
        (3, 21) => super::mou::mou_set_ptr_pos,
        (3, 22) => super::mou::mou_get_dev_status,
        (3, 25) => super::mou::mou_set_dev_status,
        (3, 26) => super::mou::mou_draw_ptr,
        (3, 29) => super::mou::mou_get_threshold,
        // ---- SESMGR ----
        (6, 8) | (6, 40) => super::sesmgr::dos_stop_session,
        (6, 9) | (6, 38) => super::sesmgr::dos_select_session,
        (6, 14) | (6, 39) => super::sesmgr::dos_set_session,
        (6, 17) | (6, 37) => super::sesmgr::dos_start_session,
        (0, 240) => super::npipe::dos_call_npipe,
        (0, 241) => super::npipe::dos_connect_npipe,
        (0, 242) => super::npipe::dos_disconnect_npipe,
        (0, 243) => super::npipe::dos_create_npipe,
        (0, 244) => super::npipe::dos_peek_npipe,
        (0, 245) => super::npipe::dos_query_nph_state,
        (0, 248) => super::npipe::dos_query_npipe_info,
        (0, 249) => super::npipe::dos_query_npipe_sem_state,
        (0, 250) => super::npipe::dos_set_nph_state,
        (0, 251) => super::npipe::dos_set_npipe_sem,
        (0, 252) => super::npipe::dos_transact_npipe,
        (0, 253) => super::npipe::dos_wait_npipe,
        (0, 44) | (0, 318) => super::module::dos_load_module,
        (0, 45) => super::module::dos16_get_proc_addr,
        (0, 46) | (0, 322) => super::module::dos_free_module,
        (0, 163) | (0, 323) => super::module::dos_query_app_type,
        (0, 321) => super::module::dos_query_proc_addr,
        (0, 586) => super::module::dos_query_proc_type,
        (0, 29) => super::timer::dos16_timer_async,
        (0, 30) => super::timer::dos16_timer_start,
        (0, 31) | (0, 290) => super::timer::dos_stop_timer,
        (0, 350) => super::timer::dos_async_timer,
        (0, 351) => super::timer::dos_start_timer,
        (0, 362) => super::sysctl::dos_tmr_query_freq,
        (0, 363) => super::sysctl::dos_tmr_query_time,
        (0, 35) => super::shmem::dos16_alloc_shr_seg,
        (0, 36) => super::shmem::dos16_get_shr_seg,
        (0, 37) => super::shmem::dos16_give_seg,
        (0, 121) => super::shmem::dos16_get_seg,
        (0, 146) => super::submem::dos16_sub_set,
        (0, 147) => super::submem::dos16_sub_alloc,
        (0, 148) => super::submem::dos16_sub_free,
        (0, 368) => super::sysstate::dos_query_sys_state,
        (0, 576) => super::thunk::thk16_unithunk,
        (0, 344) => super::submem::dos_sub_set_mem,
        (0, 345) => super::submem::dos_sub_alloc_mem,
        (0, 346) => super::submem::dos_sub_free_mem,
        (0, 347) => super::submem::dos_sub_unset_mem,
        (0, 300) => super::shmem::dos_alloc_shared_mem,
        (0, 301) => super::shmem::dos_get_named_shared_mem,
        (0, 302) => super::shmem::dos_get_shared_mem,
        (0, 303) => super::shmem::dos_give_shared_mem,
        (0, 155) => super::resource::dos16_get_resource,
        (0, 207) => super::resource::dos16_get_resource2,
        (0, 208) | (0, 353) => super::resource::dos_free_resource,
        (0, 352) => super::resource::dos_get_resource,
        (0, 572) => super::resource::dos_query_resource_size,
        (0, 873) | (0, 998) => super::module::dos_set_ext_libpath,
        (0, 874) | (0, 999) => super::module::dos_query_ext_libpath,
        (0, 53) | (0, 284) => super::ioctl::dos_dev_ioctl,
        (0, 99) => super::ioctl::dos16_dev_ioctl2,
        (0, 129) | (0, 287) => super::ioctl::dos_physical_disk,
        (0, 206) | (0, 415) => super::sysctl::dos_shutdown,
        (0, 690) => ok, // DosSMSetTitle
        (0, 712) => dos16_sm_pm_present,
        // ---- 16-bit DOSCALLS ----
        (0, 5) => dos16_exit,
        (0, 8) => dos16_get_info_seg,
        (0, 32) => dos16_sleep,
        (0, 33) => dos16_get_date_time,
        (0, 34) => dos16_alloc_seg,
        (0, 38) => dos16_realloc_seg,
        (0, 39) => dos16_free_seg,
        (0, 28) => dos16_set_date_time,
        (0, 292) => dos_set_date_time,
        (0, 40) => dos16_alloc_huge,
        (0, 41) => dos16_get_huge_shift,
        (0, 210) => ok, // DosSetVerify
        (0, 110) | (0, 203) => file::dos_delete, // DosForceDelete (no DELDIR)
        (0, 185) => file::dos16_mkdir2,
        (0, 201) | (0, 258) => file::dos_copy,
        (0, 417) => file::dos_replace_module,
        (0, 126) => dos16_size_seg,
        (0, 47) => dos16_get_mod_handle,
        (0, 48) => dos16_get_mod_name,
        (0, 49) => dos16_get_machine_mode,
        (0, 50) => dos_beep,
        (0, 56) => file::dos_reset_buffer,
        (0, 57) => file::dos_set_current_dir,
        (0, 58) => file::dos16_chg_file_ptr,
        (0, 59) => file::dos_close,
        (0, 60) => file::dos_delete,
        (0, 61) => file::dos_dup_handle,
        (0, 63) => file::dos_find_close,
        (0, 64) => file::dos16_find_first,
        (0, 65) => file::dos16_find_next,
        (0, 66) => file::dos_create_dir,
        (0, 67) => file::dos_move,
        (0, 68) => file::dos_set_file_size,
        (0, 70) => file::dos16_open,
        (0, 71) => file::dos_query_current_dir,
        (0, 72) => file::dos_query_current_disk,
        (0, 73) => file::dos_query_fh_state,
        (0, 74) => file::dos_query_file_info,
        (0, 75) => file::dos16_qfile_mode,
        (0, 76) => file::dos_query_fs_info,
        (0, 77) => file::dos_query_htype_any,
        (0, 78) => dos16_qverify,
        (0, 80) => file::dos_delete_dir,
        (0, 81) => file::dos_set_default_disk,
        (0, 82) => file::dos_set_fh_state,
        (0, 83) => file::dos_set_file_info,
        (0, 84) => file::dos16_set_file_mode,
        (0, 86) => ok, // DosSetVerify
        (0, 95) => file::dos16_open2,
        (0, 97) => file::dos_set_fs_info,
        (0, 98) => file::dos_query_path_info,
        (0, 104) => file::dos_set_path_info,
        (0, 151) => file::dos_search_path,
        (0, 182) => file::dos_query_fs_attach,
        (0, 184) => file::dos16_find_first2,
        (0, 191) | (0, 261) => file::dos_edit_name,
        (0, 204) | (0, 372) => file::dos_enum_attribute,
        (0, 85) => super::xproc::dos_set_max_fh,
        (0, 89) => dos16_set_vec,
        (0, 91) => dos16_get_env,
        (0, 92) => dos16_get_version,
        (0, 94) => dos16_get_pid,
        (0, 120) | (0, 212) => super::harderr::dos_error,
        (0, 127) => dos16_mem_avail,
        (0, 137) => file::dos16_read,
        (0, 149) => file::dos16_read_async,
        (0, 150) => file::dos16_write_async,
        (0, 138) => file::dos16_write,
        (0, 152) => dos16_scan_env,
        (0, 215) => dos16_page_size,
        // ---- 32-bit DOSCALLS ----
        (0, 209) => super::xproc::dos_set_max_fh,
        (0, 218) => file::dos_set_file_info,
        (0, 219) => file::dos_set_path_info,
        (0, 220) => file::dos_set_default_disk,
        (0, 221) => file::dos_set_fh_state,
        (0, 222) => file::dos_set_fs_info,
        (0, 223) => file::dos_query_path_info,
        (0, 224) => file::dos_query_htype_any,
        (0, 225) => dos_query_verify,
        (0, 226) => file::dos_delete_dir,
        (0, 228) => file::dos_search_path,
        (0, 227) => dos_scan_env,
        (0, 229) => dos_sleep,
        (0, 230) => dos_get_date_time,
        (0, 234) => dos_exit,
        (0, 254) => file::dos_reset_buffer,
        (0, 255) => file::dos_set_current_dir,
        (0, 256) => file::dos_set_file_ptr,
        (0, 257) => file::dos_close,
        (0, 259) => file::dos_delete,
        (0, 260) => file::dos_dup_handle,
        (0, 263) => file::dos_find_close,
        (0, 264) => file::dos_find_first,
        (0, 265) => file::dos_find_next,
        (0, 270) => file::dos_create_dir,
        (0, 271) => file::dos_move,
        (0, 272) => file::dos_set_file_size,
        (0, 273) => file::dos_open,
        (0, 274) => file::dos_query_current_dir,
        (0, 275) => file::dos_query_current_disk,
        (0, 276) => file::dos_query_fh_state,
        (0, 277) => file::dos_query_fs_attach,
        (0, 278) => file::dos_query_fs_info,
        (0, 279) => file::dos_query_file_info,
        (0, 281) => file::dos_read,
        (0, 282) => file::dos_write,
        (0, 286) => dos_beep,
        (0, 291) => nls::dos_query_cp,
        (0, 299) => dos_alloc_mem,
        (0, 304) => dos_free_mem,
        (0, 305) => dos_set_mem,
        (0, 306) => dos_query_mem,
        (0, 312) => dos_get_info_blocks,
        (0, 319) => dos_query_module_handle,
        (0, 320) => dos_query_module_name,
        (0, 348) => dos_query_sys_info,
        (0, 354) => dos_set_exception_handler,
        (0, 355) => dos_unset_exception_handler,
        (0, 382) => super::xproc::dos_set_rel_max_fh,
        (0, 425) => dos_flat_to_sel,
        (0, 981) => file::dos_open_l,
        (0, 988) => file::dos_set_file_ptr_l,
        (0, 989) => file::dos_set_file_size_l,
        (0, 426) => dos_sel_to_flat,
        (0, 423) => dos16_flat_to_sel,
        (0, 424) => dos16_sel_to_flat,
        _ => return None,
    };
    Some(h)
}

fn ok(_c: &mut Ctx) -> u32 {
    err::NO_ERROR
}

fn not_supported(_c: &mut Ctx) -> u32 {
    err::NOT_SUPPORTED
}

// ---- Process ----

fn dos16_exit(c: &mut Ctx) -> u32 {
    exit(c, c.arg(0), c.arg(1) & 0xffff)
}

fn dos_exit(c: &mut Ctx) -> u32 {
    exit(c, c.arg(0), c.arg(1))
}

fn exit(c: &mut Ctx, action: u32, code: u32) -> u32 {
    let p = c.process.clone();
    c.thread.sync_exit.store(true, core::sync::atomic::Ordering::Relaxed);
    let live = p.threads.lock().iter().filter(|t| t.state() != crate::task::thread::State::Dead).count();
    // EXIT_THREAD (0) on the last thread ends the process too.
    if action == 0 && live > 1 {
        c.thread.request_exit();
    } else {
        p.exit(0, code);
    }
    0
}

fn dos16_get_pid(c: &mut Ctx) -> u32 {
    // PIDINFO: pid, tid, pidParent.
    let a = c.ptr(0);
    c.write_u16(a, c.process.pid as u16);
    c.write_u16(a + 2, c.thread.tid as u16);
    c.write_u16(a + 4, c.process.ppid as u16);
    0
}

fn dos16_get_machine_mode(c: &mut Ctx) -> u32 {
    c.write_u8(c.ptr(0), 1); // MODE_PROTECTED
    0
}

/// OS/2 Warp 4.52 reports version 20.45.
pub const VERSION_MAJOR: u32 = 20;
pub const VERSION_MINOR: u32 = 45;

fn dos16_get_version(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), (VERSION_MAJOR << 8 | VERSION_MINOR) as u16);
    0
}

fn dos16_set_vec(c: &mut Ctx) -> u32 {
    // Exception vectors for 16-bit programs (divide, overflow, bound,
    // invalid opcode, NPX). Recorded but not yet delivered.
    let prev = c.ptr(2);
    if prev != 0 {
        c.write_u32(prev, 0);
    }
    0
}

fn dos16_get_env(c: &mut Ctx) -> u32 {
    let p = &c.process;
    c.write_u16(c.ptr(0), vm::tile_selector(p.env));
    c.write_u16(c.ptr(1), (p.cmdline - p.env) as u16);
    0
}

/// Value of an environment variable.
pub fn env_value(c: &Ctx, name: &str) -> Option<alloc::string::String> {
    let a = scan_env(c, name)?;
    c.read_str(a, 4096)
}

/// Find `name` in the environment; returns the address of its value.
fn scan_env(c: &Ctx, name: &str) -> Option<u32> {
    let mut a = c.process.env;
    loop {
        let s = c.read_str(a, 4096)?;
        if s.is_empty() {
            return None;
        }
        if let Some((k, _)) = s.split_once('=') {
            if k.eq_ignore_ascii_case(name) {
                return Some(a + k.len() as u32 + 1);
            }
        }
        a += s.len() as u32 + 1;
    }
}

fn dos_scan_env(c: &mut Ctx) -> u32 {
    let Some(name) = c.read_str(c.arg(0), 256) else { return err::INVALID_PARAMETER };
    match scan_env(c, &name) {
        Some(v) => {
            c.write_u32(c.arg(1), v);
            0
        }
        None => err::ENVVAR_NOT_FOUND,
    }
}

fn dos16_scan_env(c: &mut Ctx) -> u32 {
    let Some(name) = c.read_str(c.ptr(0), 256) else { return err::INVALID_PARAMETER };
    match scan_env(c, &name) {
        Some(v) => {
            let far = (vm::tile_selector(v) as u32) << 16 | (v & 0xffff);
            c.write_u32(c.ptr(1), far);
            0
        }
        None => err::ENVVAR_NOT_FOUND,
    }
}

fn dos_get_info_blocks(c: &mut Ctx) -> u32 {
    let ptib = c.arg(0);
    let ppib = c.arg(1);
    if ptib != 0 {
        c.write_u32(ptib, c.thread.tib());
    }
    if ppib != 0 {
        c.write_u32(ppib, c.process.info + process::info::PIB);
    }
    0
}

fn dos16_get_info_seg(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), crate::infoseg::global_selector());
    c.write_u16(c.ptr(1), vm::tile_selector(c.process.info + process::info::LINFOSEG));
    0
}

// ---- Time ----

fn dos16_sleep(c: &mut Ctx) -> u32 {
    sched::sleep_ms(c.arg(0) as u64);
    0
}

fn dos_sleep(c: &mut Ctx) -> u32 {
    sched::sleep_ms(c.arg(0) as u64);
    0
}

fn write_datetime(c: &Ctx, a: u32) -> u32 {
    let t = crate::rtc::now();
    let hundredths = ((crate::irq::uptime_ms() / 10) % 100) as u8;
    // DATETIME: hours, minutes, seconds, hundredths, day, month, year,
    // timezone (minutes from UTC, -1 = undefined), weekday.
    let mut b = [0u8; 11];
    b[0] = t.hour;
    b[1] = t.minute;
    b[2] = t.second;
    b[3] = hundredths;
    b[4] = t.day;
    b[5] = t.month;
    b[6..8].copy_from_slice(&t.year.to_le_bytes());
    b[8..10].copy_from_slice(&(-1i16).to_le_bytes());
    b[10] = t.weekday;
    if c.write(a, &b) { 0 } else { err::INVALID_ADDRESS }
}

/// DosSetDateTime(pDateTime): DATETIME as for DosGetDateTime.
fn set_datetime(c: &Ctx, a: u32) -> u32 {
    let Some(b) = c.read_vec(a, 8) else { return err::INVALID_ADDRESS };
    let year = u16::from_le_bytes([b[6], b[7]]);
    let t = crate::rtc::DateTime {
        hour: b[0],
        minute: b[1],
        second: b[2],
        day: b[4],
        month: b[5],
        year,
        weekday: crate::rtc::weekday(year, b[5], b[4]),
    };
    let days = [31, if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if t.hour > 23 || t.minute > 59 || t.second > 59 || b[3] > 99 || !(1..=12).contains(&t.month) || !(1980..=2079).contains(&year)
        || t.day == 0 || t.day > days[t.month as usize - 1]
    {
        return err::TS_DATETIME;
    }
    crate::rtc::set(&t);
    0
}

fn dos16_set_date_time(c: &mut Ctx) -> u32 {
    set_datetime(c, c.ptr(0))
}

fn dos_set_date_time(c: &mut Ctx) -> u32 {
    set_datetime(c, c.arg(0))
}

/// DosAllocHuge(cSegs, cbPartial, psel, cMaxSegs, fsAlloc): consecutive
/// tiles, so the selectors step by the huge increment (8).
fn dos16_alloc_huge(c: &mut Ctx) -> u32 {
    let (nsegs, partial, psel, flags) = (c.arg(0), c.arg(1), c.ptr(2), c.arg(4));
    if flags & 3 != 0 {
        return err::INVALID_PARAMETER; // shared huge segments: not yet
    }
    let size = nsegs * 0x10000 + partial;
    if size == 0 {
        return err::INVALID_PARAMETER;
    }
    let r = c.space().alloc(size, pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None);
    match r {
        Ok(base) => {
            if partial != 0 {
                c.space().set_seg_limit(base + nsegs * 0x10000, partial - 1);
            }
            c.write_u16(psel, vm::tile_selector(base));
            0
        }
        Err(_) => err::NOT_ENOUGH_MEMORY,
    }
}

/// DosGetHugeShift(pusShiftCount): selectors of a huge object are
/// 1 << 3 apart.
fn dos16_get_huge_shift(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), 3);
    0
}

fn dos16_get_date_time(c: &mut Ctx) -> u32 {
    write_datetime(c, c.ptr(0))
}

fn dos_get_date_time(c: &mut Ctx) -> u32 {
    write_datetime(c, c.arg(0))
}

fn dos_beep(_c: &mut Ctx) -> u32 {
    0
}

// ---- 16-bit memory ----

fn dos16_alloc_seg(c: &mut Ctx) -> u32 {
    let size = match c.arg(0) {
        0 => 0x10000,
        n => n,
    };
    let flags = c.arg(2);
    if flags & 3 != 0 {
        // SEG_GIVEABLE (1) / SEG_GETTABLE (2): shared memory.
        return match super::shmem::alloc_shared_seg(c, size, flags & 1 != 0, flags & 2 != 0) {
            Ok(sel) => {
                c.write_u16(c.ptr(1), sel);
                0
            }
            Err(e) => e,
        };
    }
    let r = c.space().alloc(size, pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None);
    match r {
        Ok(base) => {
            c.space().set_seg_limit(base, size - 1);
            c.write_u16(c.ptr(1), vm::tile_selector(base));
            0
        }
        Err(e) => e.code(),
    }
}

fn dos16_realloc_seg(c: &mut Ctx) -> u32 {
    let size = match c.arg(0) {
        0 => 0x10000,
        n => n,
    };
    let base = vm::tiled_flat(c.arg(1) as u16, 0);
    if super::tracing() {
        crate::trace!("    realloc sel {:x} size {:x}", c.arg(1), size);
    }
    let mut sp = c.space();
    let Some(r) = sp.regions.get(&base) else { return err::INVALID_ACCESS };
    let cur = r.size;
    if size > cur {
        if sp.resize(base, size).is_err() {
            return err::NOT_ENOUGH_MEMORY;
        }
    }
    sp.set_seg_limit(base, size - 1);
    0
}

fn dos16_size_seg(c: &mut Ctx) -> u32 {
    // DosSizeSeg(sel, pulSize)
    let (sel, out) = (c.arg(0) as u16, c.ptr(1));
    let d = c.space().descriptor(sel);
    if sel < 4 || d & (1 << 47) == 0 {
        return err::INVALID_ACCESS;
    }
    c.write_u32(out, vm::desc_limit(d).wrapping_add(1));
    0
}

fn dos16_free_seg(c: &mut Ctx) -> u32 {
    let base = vm::tiled_flat(c.arg(0) as u16, 0);
    match c.space().free(base) {
        Ok(()) => 0,
        Err(_) => err::INVALID_ACCESS,
    }
}

fn dos16_mem_avail(c: &mut Ctx) -> u32 {
    let free = crate::mm::phys::free_count() * 4096;
    c.write_u32(c.ptr(0), free.min(u32::MAX as u64) as u32);
    0
}

fn dos16_page_size(c: &mut Ctx) -> u32 {
    c.write_u32(c.ptr(0), 4096);
    0
}

// ---- 32-bit memory ----

fn dos_alloc_mem(c: &mut Ctx) -> u32 {
    let (ppb, cb, flags) = (c.arg(0), c.arg(1), c.arg(2));
    if flags & !(pag::READ | pag::WRITE | pag::EXECUTE | pag::GUARD | pag::COMMIT | pag::OBJ_TILE | pag::OBJ_ANY) != 0 {
        return err::INVALID_PARAMETER;
    }
    if flags & (pag::READ | pag::WRITE | pag::EXECUTE) == 0 {
        return err::INVALID_PARAMETER;
    }
    let r = c.space().alloc(cb, flags, SegKind::Data, None);
    match r {
        Ok(base) => {
            c.write_u32(ppb, base);
            0
        }
        Err(e) => e.code(),
    }
}

fn dos_free_mem(c: &mut Ctx) -> u32 {
    let base = c.arg(0);
    let r = c.space().free(base);
    match r {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

fn dos_set_mem(c: &mut Ctx) -> u32 {
    let (a, n, f) = (c.arg(0), c.arg(1), c.arg(2));
    let r = c.space().set_mem(a, n, f);
    match r {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

fn dos_query_mem(c: &mut Ctx) -> u32 {
    let (addr, pcb, pflag) = (c.arg(0), c.arg(1), c.arg(2));
    let Some(want) = c.read_u32(pcb) else { return err::INVALID_PARAMETER };
    let res = c.space().query(addr, want);
    match res {
        Some((size, flags)) => {
            c.write_u32(pcb, size);
            c.write_u32(pflag, flags);
            0
        }
        None => err::INVALID_ADDRESS,
    }
}

fn dos_flat_to_sel(c: &mut Ctx) -> u32 {
    // Register-based in real OS/2 (EAX in, EAX out); our gate passes the
    // value on the stack for _System callers.
    let a = c.f.rax as u32;
    (vm::tile_selector(a) as u32) << 16 | (a & 0xffff)
}

/// 16-bit DosFlatToSel / DosSelToFlat: EAX in, all of EAX out (the gate
/// return only replaces AX, so EAX is set here as well).
fn dos16_flat_to_sel(c: &mut Ctx) -> u32 {
    let v = dos_flat_to_sel(c);
    c.f.rax = v as u64;
    v
}

fn dos16_sel_to_flat(c: &mut Ctx) -> u32 {
    let v = dos_sel_to_flat(c);
    c.f.rax = v as u64;
    v
}

fn dos_sel_to_flat(c: &mut Ctx) -> u32 {
    let v = c.f.rax as u32;
    vm::tiled_flat((v >> 16) as u16, v as u16)
}

// ---- Exceptions ----

fn dos_set_exception_handler(c: &mut Ctx) -> u32 {
    let rec = c.arg(0);
    let tib = c.thread.tib();
    let Some(prev) = c.read_u32(tib) else { return err::INVALID_ADDRESS };
    if !c.write_u32(rec, prev) {
        return err::INVALID_ADDRESS;
    }
    c.write_u32(tib, rec);
    0
}

fn dos_unset_exception_handler(c: &mut Ctx) -> u32 {
    let rec = c.arg(0);
    let tib = c.thread.tib();
    let Some(prev) = c.read_u32(rec) else { return err::INVALID_ADDRESS };
    c.write_u32(tib, prev);
    0
}

// ---- Modules ----

fn module_name_for(c: &Ctx, hmod: u32) -> Option<String> {
    if hmod == 0 {
        return Some(c.process.exe.path.clone());
    }
    loader::by_handle(hmod as u16).map(|m| m.path.clone())
}

fn dos_query_module_name(c: &mut Ctx) -> u32 {
    let Some(name) = module_name_for(c, c.arg(0)) else { return err::INVALID_HANDLE };
    if name.len() + 1 > c.arg(1) as usize {
        return err::BUFFER_OVERFLOW;
    }
    c.write_str(c.arg(2), &name);
    0
}

fn dos16_get_mod_name(c: &mut Ctx) -> u32 {
    let Some(name) = module_name_for(c, c.arg(0)) else { return err::INVALID_HANDLE };
    if name.len() + 1 > c.arg(1) as usize {
        return err::BUFFER_OVERFLOW;
    }
    c.write_str(c.ptr(2), &name);
    0
}

fn find_module_handle(name: &str) -> Option<u16> {
    loader::find_loaded(&loader::module_name(name)).map(|m| m.hmte)
}

fn dos_query_module_handle(c: &mut Ctx) -> u32 {
    let Some(name) = c.read_str(c.arg(0), 260) else { return err::INVALID_PARAMETER };
    match find_module_handle(&name) {
        Some(h) => {
            c.write_u32(c.arg(1), h as u32);
            0
        }
        None => err::MOD_NOT_FOUND,
    }
}

fn dos16_get_mod_handle(c: &mut Ctx) -> u32 {
    let Some(name) = c.read_str(c.ptr(0), 260) else { return err::INVALID_PARAMETER };
    match find_module_handle(&name) {
        Some(h) => {
            c.write_u16(c.ptr(1), h);
            0
        }
        None => err::MOD_NOT_FOUND,
    }
}

// ---- System information ----

fn sys_info_value(i: u32) -> u32 {
    let total = crate::mm::phys::total_frames() * 4096;
    let free = crate::mm::phys::free_count() * 4096;
    match i {
        1 => 260,              // QSV_MAX_PATH_LENGTH
        2 => 16,               // QSV_MAX_TEXT_SESSIONS
        3 => 16,               // QSV_MAX_PM_SESSIONS
        4 => 0,                // QSV_MAX_VDM_SESSIONS
        5 => 3,                // QSV_BOOT_DRIVE (C:)
        6 => 1,                // QSV_DYN_PRI_VARIATION
        7 => 3,                // QSV_MAX_WAIT
        8 => 32,               // QSV_MIN_SLICE
        9 => 248,              // QSV_MAX_SLICE
        10 => 4096,            // QSV_PAGE_SIZE
        11 => VERSION_MAJOR,   // QSV_VERSION_MAJOR
        12 => VERSION_MINOR,   // QSV_VERSION_MINOR
        13 => 0,               // QSV_VERSION_REVISION
        14 => crate::irq::uptime_ms() as u32, // QSV_MS_COUNT
        15 => crate::rtc::unix_time(&crate::rtc::now()), // QSV_TIME_LOW
        16 => 0,               // QSV_TIME_HIGH
        17 => total.min(u32::MAX as u64) as u32, // QSV_TOTPHYSMEM
        18 => 0,               // QSV_TOTRESMEM
        19 => free.min(u32::MAX as u64) as u32, // QSV_TOTAVAILMEM
        20 => 0x1c00_0000,     // QSV_MAXPRMEM
        21 => 0x0c00_0000,     // QSV_MAXSHMEM
        22 => 31,              // QSV_TIMER_INTERVAL (tenths of ms)
        23 => 255,             // QSV_MAX_COMP_LENGTH
        24 => 1,               // QSV_FOREGROUND_FS_SESSION
        25 => 1,               // QSV_FOREGROUND_PROCESS
        26 => crate::smp::online_count(), // QSV_NUMPROCESSORS
        27 => 0,               // QSV_MAXHPRMEM
        28 => 0,               // QSV_MAXHSHMEM
        29 => 1000,            // QSV_MAXPROCESSES
        30 => 512,             // QSV_VIRTUALADDRESSLIMIT (MB)
        _ => 0,
    }
}

fn dos_query_sys_info(c: &mut Ctx) -> u32 {
    let (first, last, buf, cb) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    if first == 0 || last < first || last > 30 {
        return err::INVALID_PARAMETER;
    }
    if (last - first + 1) * 4 > cb {
        return err::BUFFER_OVERFLOW;
    }
    for (n, i) in (first..=last).enumerate() {
        c.write_u32(buf + 4 * n as u32, sys_info_value(i));
    }
    0
}

fn dos16_qverify(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), 0);
    0
}

fn dos_query_verify(c: &mut Ctx) -> u32 {
    c.write_u32(c.arg(0), 0);
    0
}


// ---- Signals (16-bit) ----

fn dos16_set_sig_handler(c: &mut Ctx) -> u32 {
    // DosSetSigHandler(pfnSigHandler, ppfnPrev, pfAction, fAction, usSigNum)
    let (pfn, pprev, pact, action, sig) = (c.arg(0), c.ptr(1), c.ptr(2), c.arg(3), c.arg(4));
    if !(1..=7).contains(&sig) {
        return err::INVALID_SIGNAL_NUMBER;
    }
    let mut t = c.process.signals.lock();
    let old = t[sig as usize];
    if action != 4 {
        // SIGA_ACKNOWLEDGE (4) only re-enables delivery.
        t[sig as usize] = (pfn, action);
    }
    drop(t);
    if pprev != 0 {
        c.write_u32(pprev, old.0);
    }
    if pact != 0 {
        c.write_u16(pact, old.1 as u16);
    }
    0
}

// ---- Misc ----

fn dos16_create_cs_alias(c: &mut Ctx) -> u32 {
    let sel = c.arg(0) as u16;
    let r = {
        let mut sp = c.space();
        let d = sp.descriptor(sel);
        if d & (1 << 47) == 0 {
            Err(err::INVALID_ACCESS)
        } else {
            sp.alias(vm::desc_base(d), vm::desc_limit(d), SegKind::Code16).map_err(|e| e.code())
        }
    };
    match r {
        Ok(alias) => {
            c.write_u16(c.ptr(1), alias);
            0
        }
        Err(e) => e,
    }
}

fn dos_dev_config(c: &mut Ctx) -> u32 {
    let (p, item) = if c.is16 { (c.ptr(0), c.arg(1)) } else { (c.arg(0), c.arg(1)) };
    let v: u8 = match item {
        0 => 0,    // DEVINFO_PRINTER
        1 => 1,    // DEVINFO_RS232
        2 => 1,    // DEVINFO_FLOPPY
        3 => 1,    // DEVINFO_COPROCESSOR
        4 => 0,    // DEVINFO_SUBMODEL
        5 => 0xfc, // DEVINFO_MODEL (AT)
        6 => 1,    // DEVINFO_ADAPTER (colour)
        _ => return err::INVALID_PARAMETER,
    };
    if c.write_u8(p, v) { 0 } else { err::INVALID_ADDRESS }
}

fn dos16_sm_pm_present(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), 0);
    0
}


