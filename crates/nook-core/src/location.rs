//! One-shot CoreLocation lookups for the weather widget.
//!
//! Weather always follows the Mac's location; [`crate::weather`] throttles
//! how often this is asked (at most every 15 min). It never starts continuous
//! updates — `requestLocation` once, reduced accuracy, then done. The grant
//! is keyed to the code signature and resets on ad-hoc re-sign.

use tokio::sync::oneshot;

/// Start a one-shot location request. Must be called on the main thread on
/// macOS (`CLLocationManager` is main-thread affine). The receiver completes
/// with coordinates or a UI-safe error string.
pub fn begin_request() -> oneshot::Receiver<Result<(f64, f64), String>> {
    let (tx, rx) = oneshot::channel();
    #[cfg(target_os = "macos")]
    {
        macos::start(tx);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = tx.send(Err("Location is only available on macOS.".into()));
    }
    rx
}

/// Reverse-geocode a fix to a city name via `CLGeocoder`. Main thread on
/// macOS. Resolves to `None` when there is no placemark or the lookup fails.
pub fn begin_reverse_geocode(lat: f64, lon: f64) -> oneshot::Receiver<Option<String>> {
    let (tx, rx) = oneshot::channel();
    #[cfg(target_os = "macos")]
    {
        macos::reverse_geocode(lat, lon, tx);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (lat, lon);
        let _ = tx.send(None);
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_request_degrades_without_core_location() {
        let rx = begin_request();
        let result = nook_core_runtime_block(rx);
        #[cfg(not(target_os = "macos"))]
        {
            let err = result.expect_err("linux/windows have no CoreLocation");
            assert!(err.contains("macOS"), "{err}");
        }
        let _ = result;
    }

    fn nook_core_runtime_block(
        rx: oneshot::Receiver<Result<(f64, f64), String>>,
    ) -> Result<(f64, f64), String> {
        crate::runtime().block_on(async { rx.await.unwrap_or_else(|_| Err("ended".into())) })
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::encode::{Encode, Encoding};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::AnyClass;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, AllocAnyThread, ClassType, DefinedClass};
    use objc2_foundation::MainThreadMarker;
    use std::cell::RefCell;
    use std::sync::Mutex;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CLLocationCoordinate2D {
        latitude: f64,
        longitude: f64,
    }

    unsafe impl Encode for CLLocationCoordinate2D {
        const ENCODING: Encoding =
            Encoding::Struct("CLLocationCoordinate2D", &[f64::ENCODING, f64::ENCODING]);
    }

    #[link(name = "CoreLocation", kind = "framework")]
    extern "C" {
        static kCLLocationAccuracyReduced: f64;
    }

    // CLAuthorizationStatus
    const AUTH_NOT_DETERMINED: i32 = 0;
    const AUTH_RESTRICTED: i32 = 1;
    const AUTH_DENIED: i32 = 2;
    const AUTH_AUTHORIZED_ALWAYS: i32 = 3;
    const AUTH_AUTHORIZED_WHEN_IN_USE: i32 = 4;

    struct Ivars {
        tx: RefCell<Option<oneshot::Sender<Result<(f64, f64), String>>>>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "NookWeatherLocationDelegate"]
        #[ivars = Ivars]
        struct NookWeatherLocationDelegate;

        unsafe impl NSObjectProtocol for NookWeatherLocationDelegate {}

        impl NookWeatherLocationDelegate {
            #[unsafe(method(locationManager:didUpdateLocations:))]
            fn did_update(&self, _manager: *mut AnyObject, locations: *mut AnyObject) {
                let coord = unsafe { first_coordinate(locations) };
                match coord {
                    Some(pair) => finish(self, Ok(pair)),
                    None => finish(self, Err("Location Services returned no fix.".into())),
                }
            }

            #[unsafe(method(locationManager:didFailWithError:))]
            fn did_fail(&self, _manager: *mut AnyObject, error: *mut AnyObject) {
                finish(self, Err(friendly_error(error)));
            }

            #[unsafe(method(locationManagerDidChangeAuthorization:))]
            fn did_change_auth(&self, manager: *mut AnyObject) {
                let status = unsafe { authorization_status(manager) };
                match status {
                    AUTH_AUTHORIZED_ALWAYS | AUTH_AUTHORIZED_WHEN_IN_USE => unsafe {
                        let _: () = msg_send![manager, requestLocation];
                    },
                    AUTH_DENIED | AUTH_RESTRICTED => finish(
                        self,
                        Err(
                            "Location is off or denied for openNook."
                                .into(),
                        ),
                    ),
                    _ => {}
                }
            }
        }
    );

    impl NookWeatherLocationDelegate {
        fn new(tx: oneshot::Sender<Result<(f64, f64), String>>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Ivars {
                tx: RefCell::new(Some(tx)),
            });
            unsafe { msg_send![super(this), init] }
        }
    }

    unsafe fn first_coordinate(locations: *mut AnyObject) -> Option<(f64, f64)> {
        if locations.is_null() {
            return None;
        }
        let loc: *mut AnyObject = msg_send![locations, firstObject];
        if loc.is_null() {
            return None;
        }
        let coord: CLLocationCoordinate2D = msg_send![loc, coordinate];
        Some((coord.latitude, coord.longitude))
    }

    unsafe fn authorization_status(manager: *mut AnyObject) -> i32 {
        if manager.is_null() {
            return AUTH_DENIED;
        }
        msg_send![manager, authorizationStatus]
    }

    fn friendly_error(error: *mut AnyObject) -> String {
        if error.is_null() {
            return "Location failed.".into();
        }
        let code: isize = unsafe { msg_send![error, code] };
        match code {
            1 => "Location is off or denied for openNook.".into(),
            0 => "Could not determine your location yet.".into(),
            _ => format!("Location failed ({code})."),
        }
    }

    fn finish(delegate: &NookWeatherLocationDelegate, result: Result<(f64, f64), String>) {
        if let Some(tx) = delegate.ivars().tx.borrow_mut().take() {
            let _ = tx.send(result);
        }
        if let Ok(mut live) = LIVE.lock() {
            if let Some(req) = live.take() {
                let _ = Retained::autorelease_ptr(req.manager);
                let _ = Retained::autorelease_ptr(req.delegate);
            }
        }
    }

    struct LiveRequest {
        manager: Retained<AnyObject>,
        delegate: Retained<NookWeatherLocationDelegate>,
    }

    unsafe impl Send for LiveRequest {}

    static LIVE: Mutex<Option<LiveRequest>> = Mutex::new(None);

    pub fn start(tx: oneshot::Sender<Result<(f64, f64), String>>) {
        if MainThreadMarker::new().is_none() {
            let _ = tx.send(Err(
                "System location must be requested from the main thread.".into(),
            ));
            return;
        }
        if let Ok(guard) = LIVE.lock() {
            if guard.is_some() {
                let _ = tx.send(Err("A location request is already in progress.".into()));
                return;
            }
        }

        let delegate = NookWeatherLocationDelegate::new(tx);
        unsafe {
            let manager: *mut AnyObject = msg_send![objc2::class!(CLLocationManager), new];
            if manager.is_null() {
                finish(
                    &delegate,
                    Err("Location Services are unavailable on this Mac.".into()),
                );
                return;
            }
            let Some(manager) = Retained::from_raw(manager) else {
                finish(
                    &delegate,
                    Err("Location Services are unavailable on this Mac.".into()),
                );
                return;
            };
            let accuracy = kCLLocationAccuracyReduced;
            let _: () = msg_send![&*manager, setDesiredAccuracy: accuracy];
            let _: () = msg_send![&*manager, setDelegate: &*delegate];
            let status = authorization_status(Retained::as_ptr(&manager) as *mut AnyObject);
            match status {
                AUTH_AUTHORIZED_ALWAYS | AUTH_AUTHORIZED_WHEN_IN_USE => {
                    let _: () = msg_send![&*manager, requestLocation];
                }
                AUTH_DENIED | AUTH_RESTRICTED => {
                    finish(
                        &delegate,
                        Err(
                            "Location is off or denied for openNook."
                                .into(),
                        ),
                    );
                    return;
                }
                AUTH_NOT_DETERMINED => {
                    let _: () = msg_send![&*manager, requestWhenInUseAuthorization];
                }
                _ => {
                    let _: () = msg_send![&*manager, requestWhenInUseAuthorization];
                }
            }
            if let Ok(mut live) = LIVE.lock() {
                *live = Some(LiveRequest { manager, delegate });
            }
        }
    }

    pub fn reverse_geocode(lat: f64, lon: f64, tx: oneshot::Sender<Option<String>>) {
        use block2::RcBlock;
        if MainThreadMarker::new().is_none() {
            let _ = tx.send(None);
            return;
        }
        unsafe {
            let geocoder: *mut AnyObject = msg_send![objc2::class!(CLGeocoder), new];
            let Some(geocoder) = Retained::from_raw(geocoder) else {
                let _ = tx.send(None);
                return;
            };
            let alloc: *mut AnyObject = msg_send![objc2::class!(CLLocation), alloc];
            let location: *mut AnyObject = msg_send![alloc, initWithLatitude: lat, longitude: lon];
            let Some(location) = Retained::from_raw(location) else {
                let _ = tx.send(None);
                return;
            };
            let tx = Mutex::new(Some(tx));
            // The handler holds the geocoder so it outlives the lookup; the
            // geocoder drops the handler (and itself) once it has answered.
            let keep = geocoder.clone();
            let handler = RcBlock::new(move |placemarks: *mut AnyObject, _err: *mut AnyObject| {
                let _ = &keep;
                let name = first_locality(placemarks);
                if let Ok(mut slot) = tx.lock() {
                    if let Some(tx) = slot.take() {
                        let _ = tx.send(name);
                    }
                }
            });
            let _: () = msg_send![
                &*geocoder,
                reverseGeocodeLocation: &*location,
                completionHandler: &*handler
            ];
        }
    }

    /// City of the first placemark: locality, else sub-admin area, else name.
    unsafe fn first_locality(placemarks: *mut AnyObject) -> Option<String> {
        if placemarks.is_null() {
            return None;
        }
        let placemark: *mut AnyObject = msg_send![placemarks, firstObject];
        if placemark.is_null() {
            return None;
        }
        let candidates: [*mut objc2_foundation::NSString; 3] = [
            msg_send![placemark, locality],
            msg_send![placemark, subAdministrativeArea],
            msg_send![placemark, name],
        ];
        candidates
            .into_iter()
            .filter_map(|value| value.as_ref())
            .map(|value| value.to_string())
            .find(|text| !text.trim().is_empty())
    }

    #[allow(dead_code)]
    fn _retain_class_traits() -> Option<&'static AnyClass> {
        let _ = AllocAnyThread::alloc as fn() -> Allocated<NookWeatherLocationDelegate>;
        Some(NookWeatherLocationDelegate::class())
    }
}
