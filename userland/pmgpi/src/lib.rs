//! PMGPI.DLL: the GPI entry points. PMWIN.DLL implements them (it owns the
//! presentation spaces, which are tied to windows), exporting each at
//! ordinal 4000 + its PMGPI ordinal; these entries jump there.

#![no_std]
#![no_main]

extern crate os2;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

macro_rules! forward {
    ($($name:ident = $ord:literal;)*) => {$(
        #[allow(non_snake_case)]
        mod $name {
            unsafe extern "C" {
                #[link_name = concat!("PMWIN.", stringify!($ord))]
                pub fn target();
            }
        }
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!("jmp {}", sym $name::target);
        }
    )*};
}

forward! {
    GpiAssociate = 4351;
    GpiBeginArea = 4352;
    GpiBeginPath = 4354;
    GpiBitBlt = 4355;
    GpiBox = 4356;
    GpiCharString = 4358;
    GpiCharStringAt = 4359;
    GpiCloseFigure = 4360;
    GpiCombineRegion = 4362;
    GpiConvert = 4364;
    GpiCopyMetaFile = 4365;
    GpiCreateLogFont = 4368;
    GpiCreatePS = 4369;
    GpiCreateRegion = 4370;
    GpiDeleteBitmap = 4371;
    GpiDeleteMetaFile = 4375;
    GpiDeleteSetId = 4378;
    GpiDestroyPS = 4379;
    GpiEndArea = 4385;
    GpiEndPath = 4387;
    GpiEqualRegion = 4388;
    GpiErase = 4389;
    GpiExcludeClipRectangle = 4391;
    GpiFillPath = 4392;
    GpiFullArc = 4393;
    GpiIntersectClipRectangle = 4396;
    GpiLine = 4398;
    GpiLoadBitmap = 4399;
    GpiLoadFonts = 4400;
    GpiLoadMetaFile = 4401;
    GpiMarker = 4402;
    GpiModifyPath = 4403;
    GpiMove = 4404;
    GpiOffsetClipRegion = 4405;
    GpiOffsetRegion = 4407;
    GpiPaintRegion = 4409;
    GpiPlayMetaFile = 4411;
    GpiPointArc = 4412;
    GpiPolyFillet = 4413;
    GpiPolyFilletSharp = 4414;
    GpiPolyLine = 4415;
    GpiPolyMarker = 4416;
    GpiPolySpline = 4417;
    GpiPop = 4418;
    GpiPtInRegion = 4419;
    GpiPtVisible = 4420;
    GpiQueryArcParams = 4422;
    GpiQueryAttrMode = 4423;
    GpiQueryBackColor = 4424;
    GpiQueryBackMix = 4425;
    GpiQueryBitmapHandle = 4427;
    GpiQueryBoundaryData = 4428;
    GpiQueryCharAngle = 4429;
    GpiQueryCharBox = 4430;
    GpiQueryCharDirection = 4431;
    GpiQueryCharMode = 4432;
    GpiQueryCharSet = 4433;
    GpiQueryCharShear = 4434;
    GpiQueryClipBox = 4435;
    GpiQueryClipRegion = 4436;
    GpiQueryColor = 4437;
    GpiQueryColorData = 4438;
    GpiQueryColorIndex = 4439;
    GpiQueryCp = 4440;
    GpiQueryCurrentPosition = 4441;
    GpiQueryDefCharBox = 4442;
    GpiQueryDefaultViewMatrix = 4443;
    GpiQueryDevice = 4444;
    GpiQueryDeviceBitmapFormats = 4445;
    GpiQueryDrawControl = 4446;
    GpiQueryDrawingMode = 4447;
    GpiQueryFontMetrics = 4453;
    GpiQueryLineEnd = 4457;
    GpiQueryLineJoin = 4458;
    GpiQueryLineType = 4459;
    GpiQueryLineWidth = 4460;
    GpiQueryLineWidthGeom = 4461;
    GpiQueryMarker = 4462;
    GpiQueryMarkerBox = 4463;
    GpiQueryMarkerSet = 4464;
    GpiQueryMetaFileBits = 4465;
    GpiQueryMetaFileLength = 4466;
    GpiQueryMix = 4467;
    GpiQueryModelTransformMatrix = 4468;
    GpiQueryNearestColor = 4469;
    GpiQueryNumberSetIds = 4470;
    GpiQueryPS = 4471;
    GpiQueryPageViewport = 4472;
    GpiQueryPattern = 4473;
    GpiQueryPatternRefPoint = 4474;
    GpiQueryPatternSet = 4475;
    GpiQueryPel = 4476;
    GpiQueryRGBColor = 4479;
    GpiQueryRealColors = 4480;
    GpiQueryRegionBox = 4481;
    GpiQuerySetIds = 4486;
    GpiQueryStopDraw = 4487;
    GpiQueryTextBox = 4489;
    GpiQueryViewingTransformMatrix = 4491;
    GpiQueryWidthTable = 4492;
    GpiRectInRegion = 4494;
    GpiRectVisible = 4495;
    GpiResetBoundaryData = 4497;
    GpiResetPS = 4498;
    GpiRestorePS = 4499;
    GpiSaveMetaFile = 4500;
    GpiSavePS = 4501;
    GpiSetArcParams = 4502;
    GpiSetAttrMode = 4503;
    GpiSetBackColor = 4504;
    GpiSetBackMix = 4505;
    GpiSetBitmap = 4506;
    GpiSetBitmapId = 4508;
    GpiSetCharAngle = 4509;
    GpiSetCharBox = 4510;
    GpiSetCharDirection = 4511;
    GpiSetCharMode = 4512;
    GpiSetCharSet = 4513;
    GpiSetCharShear = 4514;
    GpiSetClipPath = 4515;
    GpiSetClipRegion = 4516;
    GpiSetColor = 4517;
    GpiSetCp = 4518;
    GpiSetCurrentPosition = 4519;
    GpiSetDefaultViewMatrix = 4520;
    GpiSetDrawControl = 4521;
    GpiSetDrawingMode = 4522;
    GpiSetLineEnd = 4528;
    GpiSetLineJoin = 4529;
    GpiSetLineType = 4530;
    GpiSetLineWidth = 4531;
    GpiSetLineWidthGeom = 4532;
    GpiSetMarker = 4533;
    GpiSetMarkerBox = 4534;
    GpiSetMarkerSet = 4535;
    GpiSetMetaFileBits = 4536;
    GpiSetMix = 4537;
    GpiSetModelTransformMatrix = 4538;
    GpiSetPS = 4539;
    GpiSetPageViewport = 4540;
    GpiSetPattern = 4541;
    GpiSetPatternRefPoint = 4542;
    GpiSetPatternSet = 4543;
    GpiSetPel = 4544;
    GpiSetRegion = 4546;
    GpiSetSegmentTransformMatrix = 4549;
    GpiSetStopDraw = 4550;
    GpiStrokePath = 4554;
    GpiUnloadFonts = 4555;
    GpiWCBitBlt = 4557;
    GpiPolyLineDisjoint = 4558;
    GpiPathToRegion = 4559;
    GpiOutlinePath = 4563;
    GpiTranslate = 4564;
    GpiScale = 4565;
    GpiRotate = 4566;
    GpiQueryDefArcParams = 4567;
    GpiSetDefArcParams = 4570;
    GpiQueryBitmapParameters = 4573;
    GpiQueryLogicalFont = 4574;
    GpiDeletePalette = 4577;
    GpiSelectPalette = 4578;
    GpiQueryPalette = 4579;
    GpiCharStringPos = 4580;
    GpiCharStringPosAt = 4581;
    GpiQueryAttrs = 4583;
    GpiQueryCharStringPos = 4584;
    GpiQueryCharStringPosAt = 4585;
    GpiQueryFonts = 4586;
    GpiQueryRegionRects = 4587;
    GpiSetAttrs = 4588;
    GpiCreateLogColorTable = 4592;
    GpiQueryLogColorTable = 4593;
    GpiCreatePalette = 4594;
    GpiAnimatePalette = 4595;
    GpiSetPaletteEntries = 4596;
    GpiQueryPaletteInfo = 4597;
    GpiCreateBitmap = 4598;
    GpiQueryBitmapBits = 4599;
    GpiQueryBitmapInfoHeader = 4601;
    GpiSetBitmapBits = 4602;
    GpiDrawBits = 4603;
    DevCloseDC = 4604;
    DevEscape = 4605;
    DevQueryCaps = 4606;
    DevQueryDeviceNames = 4607;
    DevQueryHardcopyCaps = 4608;
    DevPostDeviceModes = 4609;
    DevOpenDC = 4610;
    GpiDestroyRegion = 4611;
    GpiPartialArc = 4612;
    GpiQueryCharExtra = 4613;
    GpiSetCharExtra = 4614;
    GpiQueryCharBreakExtra = 4615;
    GpiSetCharBreakExtra = 4616;
    GpiFrameRegion = 4617;
    GpiConvertWithMatrix = 4618;
    GpiLoadPublicFonts = 4622;
    GpiUnloadPublicFonts = 4623;
    GpiQueryTextAlignment = 4648;
    GpiSetTextAlignment = 4649;
    GpiCreateRoundRectRegion = 4661;
    GpiCreateEllipticRegion = 4662;
    GpiAbortPath = 4722;
}
