//! Strict one-axis candidate controls. Defaults preserve the verified vendor baseline.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CedarTuning {
    pub no_b_frames:i32,
    pub frame_package:i32,
    pub smooth:i32,
    pub display:i32,
    pub drop_b_delay:i32,
    pub poll_budget_us:i32,
    /// Backoff between drain retries while the picture has not arrived yet. The
    /// picture is noticed at the first poll after the vendor has it, so this bounds
    /// how late that poll can be. Default preserves the verified 200 us.
    pub retry_us:i32,
    pub append_aud:i32,
    pub low_delay:i32,
    /// Vendor `VeSetSpeed` request in MHz. 0 keeps the SoC default (576 on TSPS).
    pub ve_freq_mhz:i32,
    /// `VConfig.eOutputPixelFormat` request (1 = planar 420, 6 = NV12). The FBM
    /// reports the format it actually delivers, so this is a request, not a promise.
    pub pixfmt:i32,
    /// Diagnostic: duplicate the picture copy to add known memory traffic. If the
    /// vendor wait grows with it, the path is bandwidth-limited and removing the
    /// copy will speed up the hardware too.
    pub copy_twice:i32,
}
impl CedarTuning {
    pub fn from_lookup(mut get:impl FnMut(&str)->Option<String>) -> Result<Self,&'static str> {
        fn number(value:Option<String>,default:i32,min:i32,max:i32)->Result<i32,&'static str> {
            match value {
                None=>Ok(default),
                Some(v)=> match v.parse::<i32>() {
                    Ok(n) if n>=min && n<=max=>Ok(n),
                    _=>Err("cedar: invalid candidate option; booleans need 0/1, holding counts need 1/2"),
                }
            }
        }
        Ok(Self {
            no_b_frames:number(get("PUNKTFUNK_CEDAR_NO_B"),0,0,1)?,
            frame_package:number(get("PUNKTFUNK_CEDAR_FRAME_PACKAGE"),0,0,1)?,
            smooth:number(get("PUNKTFUNK_CEDAR_SMOOTH"),2,1,2)?,
            display:number(get("PUNKTFUNK_CEDAR_DISPLAY"),2,1,2)?,
            drop_b_delay:number(get("PUNKTFUNK_CEDAR_DROP_B_DELAY"),0,0,1)?,
            poll_budget_us:number(get("PUNKTFUNK_CEDAR_POLL_US"),0,0,10000)?,
            retry_us:number(get("PUNKTFUNK_CEDAR_RETRY_US"),200,0,5000)?,
            append_aud:number(get("PUNKTFUNK_CEDAR_AUD"),0,0,1)?,
            low_delay:match get("PUNKTFUNK_CEDAR_LOW_DELAY") {
                Some(v) if v=="auto"=>-1,
                v=>number(v,0,0,1)?,
            },
            ve_freq_mhz:number(get("PUNKTFUNK_CEDAR_VE_FREQ"),0,0,1152)?,
            pixfmt:number(get("PUNKTFUNK_CEDAR_PIXFMT"),1,1,6)?,
            copy_twice:number(get("PUNKTFUNK_CEDAR_COPY_TWICE"),0,0,1)?,
        })
    }
}
