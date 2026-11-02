//! SOMD.DLL's classes as IBM's (generated from a study of IBM's SOMD.DLL:
//! each class's parents, metaclass, version, instance data size and class
//! data order). Do not edit: the generator is described in docs/WPS.md.

use crate::ClassDef;

pub static COMMADDRESS: ClassDef = ClassDef {
    name: "CommAddress",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 100,
    methods: &["", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "recvConnect", "", "verifySequence", "deleteConnection", "recvDisconnect", "deleteAllConnections", "freeMessageQueue", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "setSecInfo", "getSecInfo", "getDisconnectMsg"],
    procs: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46],
};

pub static COMMBINDING: ClassDef = ClassDef {
    name: "CommBinding",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 44,
    methods: &["", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "isBindingStale"],
    procs: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34],
};

pub static IMPLDATABASE: ClassDef = ClassDef {
    name: "ImplDatabase",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 8,
    methods: &["impldb_add_impl", "impldb_delete_impl", "impldb_update_impl", "impldb_add_class", "impldb_delete_class", "impldb_delete_class_from_impl", "impldb_clean_classes_from_impl", "impldb_get_impl", "impldb_get_impl_from_alias", "impldb_get_classes_from_impl", "impldb_locate_impl", "impldb_locate_class", "impldb_ir_compare", "impldb_print_impls", "impldb_print_aliases", "impldb_print_classes", "impldb_implrep_store", "impldb_implrep_restore", "impldb_implrep_free", "impldb_get_aliases", "impldb_get_classes", "impldb_get_aliases_from_class", "impldb_get_number_of_impldefs"],
    procs: &[],
};

pub static LOCATIONSERVICE: ClassDef = ClassDef {
    name: "LocationService",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 24,
    methods: &["", "", "", "", "", "", "", "", "", "", "", "", "delete_binding_in_cache"],
    procs: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

pub static LOCSERVD: ClassDef = ClassDef {
    name: "LocServD",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 16,
    methods: &["", "", "", "", "", "", "", "", "LSDIsImplEnabled", "", ""],
    procs: &[0, 1, 2, 3, 4, 5, 6, 7, 9, 10],
};

pub static REQUESTMSG: ClassDef = ClassDef {
    name: "RequestMsg",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 112,
    methods: &["", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", ""],
    procs: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32],
};

pub static UUID: ClassDef = ClassDef {
    name: "Uuid",
    parents: &["SOMUTStringId"],
    meta: None,
    major: 2,
    minor: 1,
    data: 16,
    methods: &["somutSetIdNil", "somutEqualsNil"],
    procs: &[],
};

pub static BOA: ClassDef = ClassDef {
    name: "BOA",
    parents: &["SOMObject"],
    meta: Some("SOMMSingleInstance"),
    major: 2,
    minor: 1,
    data: 0,
    methods: &["create", "dispose", "get_id", "change_implementation", "get_principal", "set_exception", "impl_is_ready", "deactivate_impl", "obj_is_ready", "deactivate_obj", "activate_impl", "activate_obj"],
    procs: &[],
};

pub static SOMOA: ClassDef = ClassDef {
    name: "SOMOA",
    parents: &["BOA"],
    meta: None,
    major: 2,
    minor: 1,
    data: 28,
    methods: &["execute_next_request", "execute_request_loop", "create_constant", "change_id", "create_SOM_ref", "get_SOM_object", "activate_impl_failed", "interrupt_server"],
    procs: &[],
};

pub static CONTEXT: ClassDef = ClassDef {
    name: "Context",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 12,
    methods: &["set_one_value", "set_values", "get_values", "delete_values", "create_child", "destroy", "_get_context_name", "_set_context_name", "_get_context_parent", "_set_context_parent", "_get_properties", "_set_properties"],
    procs: &[],
};

pub static IMPLEMENTATIONDEF: ClassDef = ClassDef {
    name: "ImplementationDef",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 4,
    methods: &["_set_impl_id", "_get_impl_id", "_set_impl_alias", "_get_impl_alias", "_set_impl_program", "_get_impl_program", "_set_impl_flags", "_get_impl_flags", "_set_impl_server_class", "_get_impl_server_class", "_set_impl_socket_class", "_get_impl_socket_class", "_set_impl_refdata_file", "_get_impl_refdata_file", "_set_impl_refdata_bkup", "_get_impl_refdata_bkup", "_set_impl_hostname", "_get_impl_hostname", "_set_impl_def_struct", "_get_impl_def_struct", "get_impl_winserver_handle"],
    procs: &[],
};

pub static PRINCIPAL: ClassDef = ClassDef {
    name: "Principal",
    parents: &["SOMObject"],
    meta: None,
    major: 1,
    minor: 0,
    data: 8,
    methods: &["_set_userName", "_get_userName", "_set_hostName", "_get_hostName"],
    procs: &[],
};

pub static IMPLREPOSITORY: ClassDef = ClassDef {
    name: "ImplRepository",
    parents: &["SOMObject"],
    meta: Some("SOMMSingleInstance"),
    major: 2,
    minor: 1,
    data: 4,
    methods: &["find_impldef", "find_impldef_by_alias", "find_impldef2", "find_impldef_by_alias2", "find_impldef_by_class", "add_impldef", "update_impldef", "delete_impldef", "add_class_to_impldef", "remove_class_from_impldef", "remove_class_from_all", "find_classes_by_impldef", "implr_get_impl", "implr_search_impl", "implr_search_class", "implr_update_impl", "find_all_impldefs"],
    procs: &[],
};

pub static NVLIST: ClassDef = ClassDef {
    name: "NVList",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 12,
    methods: &["add_item", "remove_item", "free", "free_memory", "get_count", "set_item", "get_item", "get_item_by_name"],
    procs: &[],
};

pub static OBJECTMGR: ClassDef = ClassDef {
    name: "ObjectMgr",
    parents: &["SOMObject"],
    meta: Some("SOMMSingleInstance"),
    major: 2,
    minor: 1,
    data: 0,
    methods: &["somdNewObject", "somdGetIdFromObject", "somdGetObjectFromId", "somdReleaseObject", "somdDestroyObject"],
    procs: &[],
};

pub static SOMDOBJECTMGR: ClassDef = ClassDef {
    name: "SOMDObjectMgr",
    parents: &["ObjectMgr"],
    meta: None,
    major: 2,
    minor: 1,
    data: 8,
    methods: &["somdFindServer", "somdFindServerByName", "somdFindServersByClass", "somdFindAnyServerByClass", "_get_somd21somFree", "_set_somd21somFree"],
    procs: &[],
};

pub static ORB: ClassDef = ClassDef {
    name: "ORB",
    parents: &["SOMObject"],
    meta: Some("SOMMSingleInstance"),
    major: 2,
    minor: 1,
    data: 0,
    methods: &["object_to_string", "string_to_object", "create_list", "create_operation_list", "get_default_context", "object_to_binary", "binary_to_object"],
    procs: &[5, 6],
};

pub static REQUEST: ClassDef = ClassDef {
    name: "Request",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 36,
    methods: &["add_arg", "invoke", "send", "get_response", "destroy", "_get_objref", "_set_objref", "_get_ctx", "_set_ctx", "_get_operation", "_set_operation", "_get_argument_list", "_set_argument_list", "_get_return_result", "_set_return_result", "_get_req_flags", "_set_req_flags", "_get_env", "_set_env"],
    procs: &[],
};

pub static SOMDSERVER: ClassDef = ClassDef {
    name: "SOMDServer",
    parents: &["SOMObject"],
    meta: Some("SOMMSingleInstance"),
    major: 2,
    minor: 1,
    data: 0,
    methods: &["somdSOMObjFromRef", "somdRefFromSOMObj", "somdCreateObj", "somdDeleteObj", "somdGetClassObj", "somdDispatchMethod", "somdObjReferencesCached"],
    procs: &[],
};

pub static SOMDSERVERMGR: ClassDef = ClassDef {
    name: "SOMDServerMgr",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 0,
    methods: &["somdShutdownServer", "somdStartServer", "somdRestartServer", "somdListServer", "somdDisableServer", "somdEnableServer", "somdIsServerEnabled"],
    procs: &[],
};

pub static SOMDOBJECT: ClassDef = ClassDef {
    name: "SOMDObject",
    parents: &["SOMObject"],
    meta: None,
    major: 2,
    minor: 1,
    data: 28,
    methods: &["get_implementation", "get_interface", "set_to_nil", "is_nil", "is_SOM_ref", "is_constant", "is_proxy", "duplicate", "release", "create_request", "create_request_args", "_get_somd_tag", "_get_somd_flags", "_set_somd_flags", "_get_somd_impl", "_set_somd_impl", "_get_somd_rid", "_set_somd_rid", "_get_somd_id", "_set_somd_id"],
    procs: &[3],
};

pub static SOMDCLIENTPROXY: ClassDef = ClassDef {
    name: "SOMDClientProxy",
    parents: &["SOMDObject"],
    meta: Some("SOMDMetaproxy"),
    major: 2,
    minor: 1,
    data: 20,
    methods: &["somdTargetFree", "somdTargetGetClass", "somdTargetGetClassName", "somdProxyFree", "somdProxyGetClass", "somdProxyGetClassName", "set_binding", "get_binding", "reset_binding", "_get_ctx_seq", "_set_ctx_seq", "somdReleaseResources", "_get_memoryMgr"],
    procs: &[6, 7, 8],
};

pub static SOMDMETAPROXY: ClassDef = ClassDef {
    name: "SOMDMetaproxy",
    parents: &["SOMMCooperativeSistered"],
    meta: None,
    major: 0,
    minor: 0,
    data: 0,
    methods: &[],
    procs: &[],
};

pub static TSIDENTIFICATION: ClassDef = ClassDef {
    name: "TSIdentification",
    parents: &["SOMObject"],
    meta: Some("SOMMSingleInstance"),
    major: 2,
    minor: 1,
    data: 0,
    methods: &["identify_sender", "identify_receiver", "_get_sender", "_get_receiver"],
    procs: &[],
};

pub static IMPLREPDIST: ClassDef = ClassDef {
    name: "ImplRepDist",
    parents: &["ImplRepository"],
    meta: None,
    major: 2,
    minor: 1,
    data: 0,
    methods: &["impldst_create_impl_dst", "impldst_apply_impl_dst", "impldst_find_all_selids", "impldst_find_impldef_by_selid", "impldst_find_selids_by_impldef", "impldst_add_selid_to_impldef", "impldst_remove_selid_from_impldef", "impldst_remove_selid_from_all"],
    procs: &[],
};
