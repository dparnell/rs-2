//! Checks of rs2's SOMUC.DLL (SOM's utility classes), which OpenDoc's
//! Apple-event terminology classes are built on.

use crate::{check, ev};

unsafe extern "C" {
    #[link_name = "SOMUC.26"]
    fn somf_MCollectibleNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMUC.37"]
    static somf_MCollectibleClassData: [usize; 7];
    #[link_name = "SOMUC.56"]
    fn somf_TCollectibleLongNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMUC.66"]
    static somf_TCollectibleLongClassData: [usize; 4];
    #[link_name = "SOMUC.71"]
    fn somf_TDequeNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMUC.75"]
    static somf_TDequeClassData: [usize; 16];
    #[link_name = "SOMUC.29"]
    static somf_TCollectionClassData: [usize; 12];
    #[link_name = "SOMUC.25"]
    static somf_TSequenceClassData: [usize; 7];
    #[link_name = "SOMUC.22"]
    static somf_TIteratorClassData: [usize; 4];
    #[link_name = "SOMUC.35"]
    fn somf_TDictionaryNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMUC.33"]
    static somf_TDictionaryClassData: [usize; 24];
    #[link_name = "SOMUC.47"]
    fn somf_TSetNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMUC.55"]
    static somf_TSetClassData: [usize; 22];
    #[link_name = "SOMUC.16"]
    fn somf_TPrimitiveLinkedListNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMUC.20"]
    static somf_TPrimitiveLinkedListClassData: [usize; 14];
    #[link_name = "SOMUC.4"]
    fn somf_MLinkableNewClass(major: i32, minor: i32) -> usize;
}

macro_rules! cd {
    ($a:ident) => {
        unsafe { (&raw const $a) as *const usize }
    };
}

fn m(cd: *const usize, i: usize, args: &[usize]) -> usize {
    somsup::call(cd, i, args)
}

fn long(v: i32) -> usize {
    let o = somsup::new_object(unsafe { somf_TCollectibleLongNewClass(2, 1) });
    m(cd!(somf_TCollectibleLongClassData), 2, &[o, ev(), v as usize]);
    o
}

fn val(o: usize) -> i32 {
    if o == 0 { -1 } else { m(cd!(somf_TCollectibleLongClassData), 0, &[o, ev()]) as i32 }
}

pub fn somuc_test() {
    let e = ev();
    // MCollectible
    let a = somsup::new_object(unsafe { somf_MCollectibleNewClass(2, 1) });
    let b = somsup::new_object(unsafe { somf_MCollectibleNewClass(2, 1) });
    let mc = cd!(somf_MCollectibleClassData);
    check("SOMUC: somf_MCollectible objects", a != 0 && b != 0);
    check(
        "somfIsSame, somfIsEqual and somfIsNotEqual (identity by default)",
        m(mc, 4, &[a, e, a]) & 0xff != 0 && m(mc, 3, &[a, e, b]) & 0xff == 0 && m(mc, 5, &[a, e, b]) & 0xff != 0,
    );
    check("somfHash is the object's address by default", m(mc, 2, &[a, e]) == a);
    // TCollectibleLong: equal by value, cloned with its value
    let (x, y) = (long(7), long(7));
    let c = m(mc, 0, &[x, e]);
    check("somf_TCollectibleLong: equal values are equal, a clone has the value", m(mc, 3, &[x, e, y]) & 0xff != 0 && val(c) == 7 && c != x);
    // TDeque
    let dq = somsup::new_object(unsafe { somf_TDequeNewClass(2, 1) });
    let d = cd!(somf_TDequeClassData);
    let tc = cd!(somf_TCollectionClassData);
    let ts = cd!(somf_TSequenceClassData);
    m(d, 2, &[dq, e, long(2)]); // AddLast
    m(d, 3, &[dq, e, long(1)]); // AddFirst
    m(d, 2, &[dq, e, long(3)]);
    check_eq_seq("somf_TDeque: AddFirst/AddLast, Count, First, Last", m(tc, 5, &[dq, e]), val(m(ts, 0, &[dq, e])), val(m(ts, 3, &[dq, e])), (3, 1, 3));
    let it = m(tc, 7, &[dq, e]);
    let ti = cd!(somf_TIteratorClassData);
    let (i1, i2, i3, i4) = (val(m(ti, 1, &[it, e])), val(m(ti, 0, &[it, e])), val(m(ti, 0, &[it, e])), m(ti, 0, &[it, e]));
    check("its iterator gives the items in order", (i1, i2, i3, i4) == (1, 2, 3, 0));
    let found = m(tc, 6, &[dq, e, long(2)]);
    check("somfMember finds an equal item (the test function: somfIsEqual)", val(found) == 2);
    let popped = val(m(d, 9, &[dq, e])); // Pop (the last)
    let first = val(m(d, 5, &[dq, e])); // RemoveFirst
    check("Pop and RemoveFirst", popped == 3 && first == 1 && m(tc, 5, &[dq, e]) == 1);
    // TDictionary: values by equal keys
    let dict = somsup::new_object(unsafe { somf_TDictionaryNewClass(2, 1) });
    let dd = cd!(somf_TDictionaryClassData);
    m(dd, 7, &[dict, e, long(10), long(100)]); // AddKeyValuePairMM
    m(dd, 7, &[dict, e, long(20), long(200)]);
    let old = m(dd, 7, &[dict, e, long(10), long(101)]);
    check(
        "somf_TDictionary: ValueAt by an equal key, a pair replaced",
        val(m(dd, 2, &[dict, e, long(20)])) == 200 && val(m(dd, 2, &[dict, e, long(10)])) == 101 && val(old) == 100 && m(tc, 5, &[dict, e]) == 2,
    );
    check("KeyAtM", val(m(dd, 4, &[dict, e, long(200)])) == 20);
    // TSet: no duplicates, union
    let s1 = somsup::new_object(unsafe { somf_TSetNewClass(2, 1) });
    let s2 = somsup::new_object(unsafe { somf_TSetNewClass(2, 1) });
    for v in [1, 2, 2, 3] {
        m(tc, 0, &[s1, e, long(v)]);
    }
    for v in [3, 4] {
        m(tc, 0, &[s2, e, long(v)]);
    }
    let n1 = m(tc, 5, &[s1, e]);
    m(cd!(somf_TSetClassData), 4, &[s1, e, s2]); // UnionS
    check("somf_TSet: equal items once, UnionS", n1 == 3 && m(tc, 5, &[s1, e]) == 4);
    // TPrimitiveLinkedList of MLinkables
    let pl = somsup::new_object(unsafe { somf_TPrimitiveLinkedListNewClass(2, 1) });
    let lcls = unsafe { somf_MLinkableNewClass(2, 1) };
    let (l1, l2, l3) = (somsup::new_object(lcls), somsup::new_object(lcls), somsup::new_object(lcls));
    let p = cd!(somf_TPrimitiveLinkedListClassData);
    m(p, 8, &[pl, e, l2]); // AddLast
    m(p, 7, &[pl, e, l1]); // AddFirst
    m(p, 6, &[pl, e, l2, l3]); // AddAfter
    let ok = m(p, 0, &[pl, e]) == 3 && m(p, 11, &[pl, e]) == l1 && m(p, 9, &[pl, e, l1]) == l2 && m(p, 12, &[pl, e]) == l3;
    m(p, 1, &[pl, e, l2]); // Remove
    check("somf_TPrimitiveLinkedList: links added first, last, after; removed", ok && m(p, 9, &[pl, e, l1]) == l3 && m(p, 0, &[pl, e]) == 2);
    crate::ok_ev("SOMUC");
}

fn check_eq_seq(name: &str, count: usize, first: i32, last: i32, want: (usize, i32, i32)) {
    check(name, (count, first, last) == want);
}
