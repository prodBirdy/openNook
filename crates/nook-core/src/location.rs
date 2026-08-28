//! Opt-in one-shot CoreLocation lookup.
//!
//! Manual city entry is the default weather path (no TCC). This module is
//! only used when the user taps "Use system location". It never starts
//! continuous updates — `requestLocation` once, reduced accuracy, then done.
//! The grant is keyed to the code signature and resets on ad-hoc re-sign.

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
        let _ = tx.send(Err(
            "System location is only available on macOS. Enter a city instead.".into(),
        ));
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
        crate::runtime()
            .block_on(async { rx.await.unwrap_or_else(|_| Err("ended".into())) })
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::encode::{Encode, Encoding};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::runtime::AnyClass;
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
        const ENCODING: Encoding = Encoding::Struct(
            "CLLocationCoordinate2D",
            &[f64::ENCODING, f64::ENCODING],
        );
    }

    #[link(name = "CoreLocation", kind = "framework")]
    extern "C" {
        static kCLLocationAccuracyReduced: f64;
    }

    // CLAuthorizationStatus
    const AUTH_NOT_DETERMINED: i64 = 0;
    const AUTH_RESTRICTED: i64 = 1;
    const AUTH_DENIED: i64 = 2;
    const AUTH_AUTHORIZED_ALWAYS: i64 = 3;
    const AUTH_AUTHORIZED_WHEN_IN_USE: i64 = 4;

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
                            "Location is off or denied. Enter a city, or enable Location Services."
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

    unsafe fn authorization_status(manager: *mut AnyObject) -> i64 {
        if manager.is_null() {
            return AUTH_DENIED;
        }
        msg_send![manager, authorizationStatus]
    }

    fn friendly_error(error: *mut AnyObject) -> String {
        if error.is_null() {
            return "Location failed. Enter a city instead.".into();
        }
        let code: isize = unsafe { msg_send![error, code] };
        match code {
            1 => "Location is off or denied. Enter a city, or enable Location Services.".into(),
            0 => "Could not determine location. Try again, or enter a city.".into(),
            _ => format!("Location failed ({code}). Enter a city instead."),
        }
    }

    fn finish(delegate: &NookWeatherLocationDelegate, result: Result<(f64, f64), String>) {
        if let Some(tx) = delegate.ivars().tx.borrow_mut().take() {
            let _ = tx.send(result);
        }
        if let Ok(mut live) = LIVE.lock() {
            *live = None;
        }
    }

    struct LiveRequest {
        _manager: Retained<AnyObject>,
        _delegate: Retained<NookWeatherLocationDelegate>,
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
            let accuracy = kCLLocationAccuracyReduced;
            let _: () = msg_send![manager, setDesiredAccuracy: accuracy];
            let _: () = msg_send![manager, setDelegate: &*delegate];
            let status = authorization_status(manager);
            match status {
                AUTH_AUTHORIZED_ALWAYS | AUTH_AUTHORIZED_WHEN_IN_USE => {
                    let _: () = msg_send![manager, requestLocation];
                }
                AUTH_DENIED | AUTH_RESTRICTED => {
                    finish(
                        &delegate,
                        Err(
                            "Location is off or denied. Enter a city, or enable Location Services."
                                .into(),
                        ),
                    );
                    return;
                }
                AUTH_NOT_DETERMINED => {
                    let _: () = msg_send![manager, requestWhenInUseAuthorization];
                }
                _ => {
                    let _: () = msg_send![manager, requestWhenInUseAuthorization];
                }
            }
            let Some(manager) = Retained::from_raw(manager) else {
                finish(
                    &delegate,
                    Err("Location Services are unavailable on this Mac.".into()),
                );
                return;
            };
            if let Ok(mut live) = LIVE.lock() {
                *live = Some(LiveRequest {
                    _manager: manager,
                    _delegate: delegate,
                });
            }
        }
    }

    #[allow(dead_code)]
    fn _retain_class_traits() -> Option<&'static AnyClass> {
        let _ = AllocAnyThread::alloc as fn() -> Allocated<NookWeatherLocationDelegate>;
        Some(NookWeatherLocationDelegate::class())
    }
}
