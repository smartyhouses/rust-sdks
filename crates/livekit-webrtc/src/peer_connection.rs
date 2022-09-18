use cxx::UniquePtr;
use libwebrtc_sys::jsep as sys_jsep;
use libwebrtc_sys::peer_connection as sys_pc;
use log::trace;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::data_channel::DataChannel;
use crate::jsep::{IceCandidate, SessionDescription};
use crate::media_stream::MediaStream;
use crate::rtc_error::RTCError;
use crate::rtp_receiver::RtpReceiver;
use crate::rtp_transceiver::RtpTransceiver;

pub use libwebrtc_sys::peer_connection::ffi::IceConnectionState;
pub use libwebrtc_sys::peer_connection::ffi::IceGatheringState;
pub use libwebrtc_sys::peer_connection::ffi::PeerConnectionState;
pub use libwebrtc_sys::peer_connection::ffi::RTCOfferAnswerOptions;
pub use libwebrtc_sys::peer_connection::ffi::SignalingState;

#[derive(Error, Debug)]
pub enum SdpError {
    #[error("recv failure: {0}")]
    RecvError(String),
    #[error("internal libwebrtc error")]
    RTCError(#[from] RTCError),
}

pub struct PeerConnection {
    cxx_handle: UniquePtr<sys_pc::ffi::PeerConnection>,
    observer: Box<InternalObserver>,
}

impl PeerConnection {
    pub(crate) fn new(
        cxx_handle: UniquePtr<sys_pc::ffi::PeerConnection>,
        observer: Box<InternalObserver>,
    ) -> Self {
        Self {
            cxx_handle,
            observer,
        }
    }

    pub async fn create_offer(&mut self) -> Result<SessionDescription, SdpError> {
        let (tx, mut rx) = mpsc::channel(1);

        let wrapper =
            sys_jsep::CreateSdpObserverWrapper::new(Box::new(InternalCreateSdpObserver { tx }));
        let native_wrapper = sys_jsep::ffi::create_native_create_sdp_observer(Box::new(wrapper));

        self.cxx_handle
            .pin_mut()
            .create_offer(native_wrapper, RTCOfferAnswerOptions::default());

        match rx.recv().await {
            Some(value) => value.map_err(Into::into),
            None => Err(SdpError::RecvError("channel closed".to_string())),
        }
    }

    pub async fn create_answer(&mut self) -> Result<SessionDescription, SdpError> {
        let (tx, mut rx) = mpsc::channel(1);

        let wrapper =
            sys_jsep::CreateSdpObserverWrapper::new(Box::new(InternalCreateSdpObserver { tx }));
        let native_wrapper = sys_jsep::ffi::create_native_create_sdp_observer(Box::new(wrapper));

        self.cxx_handle
            .pin_mut()
            .create_answer(native_wrapper, RTCOfferAnswerOptions::default());

        match rx.recv().await {
            Some(value) => value.map_err(Into::into),
            None => Err(SdpError::RecvError("channel closed".to_string())),
        }
    }

    pub async fn set_local_description(
        &mut self,
        desc: SessionDescription,
    ) -> Result<(), SdpError> {
        let (tx, mut rx) = mpsc::channel(1);
        let wrapper =
            sys_jsep::SetLocalSdpObserverWrapper::new(Box::new(InternalSetLocalSdpObserver { tx }));
        let native_wrapper = sys_jsep::ffi::create_native_set_local_sdp_observer(Box::new(wrapper));

        self.cxx_handle
            .pin_mut()
            .set_local_description(desc.release(), native_wrapper);

        match rx.recv().await {
            Some(value) => value.map_err(Into::into),
            None => Err(SdpError::RecvError("channel closed".to_string())),
        }
    }

    pub async fn set_remote_description(
        &mut self,
        desc: SessionDescription,
    ) -> Result<(), SdpError> {
        let (tx, mut rx) = mpsc::channel(1);
        let wrapper =
            sys_jsep::SetRemoteSdpObserverWrapper::new(Box::new(InternalSetRemoteSdpObserver {
                tx,
            }));
        let native_wrapper =
            sys_jsep::ffi::create_native_set_remote_sdp_observer(Box::new(wrapper));

        self.cxx_handle
            .pin_mut()
            .set_remote_description(desc.release(), native_wrapper);

        match rx.recv().await {
            Some(value) => value.map_err(Into::into),
            None => Err(SdpError::RecvError("channel closed".to_string())),
        }
    }

    pub fn close(&mut self) {
        self.cxx_handle.pin_mut().close();
    }

    pub fn on_signaling_change(&mut self, handler: OnSignalingChangeHandler) {
        *self.observer.on_signaling_change_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_add_stream(&mut self, handler: OnAddStreamHandler) {
        *self.observer.on_add_stream_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_remove_stream(&mut self, handler: OnRemoveStreamHandler) {
        *self.observer.on_remove_stream_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_data_channel(&mut self, handler: OnDataChannelHandler) {
        *self.observer.on_data_channel_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_renegotiation_needed(&mut self, handler: OnRenegotiationNeededHandler) {
        *self
            .observer
            .on_renegotiation_needed_handler
            .lock()
            .unwrap() = Some(handler);
    }

    pub fn on_ice_connection_change(&mut self, handler: OnIceConnectionChangeHandler) {
        *self
            .observer
            .on_ice_connection_change_handler
            .lock()
            .unwrap() = Some(handler);
    }

    pub fn on_standardized_ice_connection_change(
        &mut self,
        handler: OnStandardizedIceConnectionChangeHandler,
    ) {
        *self
            .observer
            .on_standardized_ice_connection_change_handler
            .lock()
            .unwrap() = Some(handler);
    }

    pub fn on_connection_change(&mut self, handler: OnConnectionChangeHandler) {
        *self.observer.on_connection_change_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_ice_gathering_change(&mut self, handler: OnIceGatheringChangeHandler) {
        *self
            .observer
            .on_ice_gathering_change_handler
            .lock()
            .unwrap() = Some(handler);
    }

    pub fn on_ice_candidate(&mut self, handler: OnIceCandidateHandler) {
        *self.observer.on_ice_candidate_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_ice_candidate_error(&mut self, handler: OnIceCandidateErrorHandler) {
        *self.observer.on_ice_candidate_error_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_ice_candidates_removed(&mut self, handler: OnIceCandidatesRemovedHandler) {
        *self
            .observer
            .on_ice_candidates_removed_handler
            .lock()
            .unwrap() = Some(handler);
    }

    pub fn on_ice_connection_receiving_change(
        &mut self,
        handler: OnIceConnectionReceivingChangeHandler,
    ) {
        *self
            .observer
            .on_ice_connection_receiving_change_handler
            .lock()
            .unwrap() = Some(handler);
    }

    pub fn on_ice_selected_candidate_pair_changed(
        &mut self,
        handler: OnIceSelectedCandidatePairChangedHandler,
    ) {
        *self
            .observer
            .on_ice_selected_candidate_pair_changed_handler
            .lock()
            .unwrap() = Some(handler);
    }

    pub fn on_add_track(&mut self, handler: OnAddTrackHandler) {
        *self.observer.on_add_track_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_track(&mut self, handler: OnTrackHandler) {
        *self.observer.on_track_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_remove_track(&mut self, handler: OnRemoveTrackHandler) {
        *self.observer.on_remove_track_handler.lock().unwrap() = Some(handler);
    }

    pub fn on_interesting_usage(&mut self, handler: OnInterestingUsageHandler) {
        *self.observer.on_interesting_usage_handler.lock().unwrap() = Some(handler);
    }
}

// CreateSdpObserver

struct InternalCreateSdpObserver {
    tx: mpsc::Sender<Result<SessionDescription, RTCError>>,
}

impl sys_jsep::CreateSdpObserver for InternalCreateSdpObserver {
    fn on_success(
        &self,
        session_description: UniquePtr<libwebrtc_sys::jsep::ffi::SessionDescription>,
    ) {
        self.tx
            .blocking_send(Ok(SessionDescription::new(session_description)))
            .unwrap();
    }

    fn on_failure(&self, error: RTCError) {
        self.tx.blocking_send(Err(error)).unwrap();
    }
}

// SetLocalSdpObserver

struct InternalSetLocalSdpObserver {
    tx: mpsc::Sender<Result<(), RTCError>>,
}

impl sys_jsep::SetLocalSdpObserver for InternalSetLocalSdpObserver {
    fn on_set_local_description_complete(&self, error: RTCError) {
        let res = if error.ok() { Ok(()) } else { Err(error) };
        self.tx.blocking_send(res).unwrap();
    }
}

// SetRemoteSdpObserver

struct InternalSetRemoteSdpObserver {
    tx: mpsc::Sender<Result<(), RTCError>>,
}

impl sys_jsep::SetRemoteSdpObserver for InternalSetRemoteSdpObserver {
    fn on_set_remote_description_complete(&self, error: RTCError) {
        let res = if error.ok() { Ok(()) } else { Err(error) };
        self.tx.blocking_send(res).unwrap();
    }
}

// PeerConnectionObserver

// TODO(theomonnom) Should we return futures?
pub type OnSignalingChangeHandler = Box<dyn FnMut(SignalingState) + Send + Sync>;
pub type OnAddStreamHandler = Box<dyn FnMut(MediaStream) + Send + Sync>;
pub type OnRemoveStreamHandler = Box<dyn FnMut(MediaStream) + Send + Sync>;
pub type OnDataChannelHandler = Box<dyn FnMut(DataChannel) + Send + Sync>;
pub type OnRenegotiationNeededHandler = Box<dyn FnMut() + Send + Sync>;
pub type OnNegotiationNeededEventHandler = Box<dyn FnMut(u32) + Send + Sync>;
pub type OnIceConnectionChangeHandler = Box<dyn FnMut(IceConnectionState) + Send + Sync>;
pub type OnStandardizedIceConnectionChangeHandler =
    Box<dyn FnMut(IceConnectionState) + Send + Sync>;
pub type OnConnectionChangeHandler = Box<dyn FnMut(PeerConnectionState) + Send + Sync>;
pub type OnIceGatheringChangeHandler = Box<dyn FnMut(IceGatheringState) + Send + Sync>;
pub type OnIceCandidateHandler = Box<dyn FnMut(IceCandidate) + Send + Sync>;
pub type OnIceCandidateErrorHandler =
    Box<dyn FnMut(String, i32, String, i32, String) + Send + Sync>;
pub type OnIceCandidatesRemovedHandler = Box<dyn FnMut(Vec<IceCandidate>) + Send + Sync>;
pub type OnIceConnectionReceivingChangeHandler = Box<dyn FnMut(bool) + Send + Sync>;
pub type OnIceSelectedCandidatePairChangedHandler =
    Box<dyn FnMut(libwebrtc_sys::peer_connection::ffi::CandidatePairChangeEvent) + Send + Sync>;
pub type OnAddTrackHandler = Box<dyn FnMut(RtpReceiver, Vec<MediaStream>) + Send + Sync>;
pub type OnTrackHandler = Box<dyn FnMut(RtpTransceiver) + Send + Sync>;
pub type OnRemoveTrackHandler = Box<dyn FnMut(RtpReceiver) + Send + Sync>;
pub type OnInterestingUsageHandler = Box<dyn FnMut(i32) + Send + Sync>;

pub(crate) struct InternalObserver {
    on_signaling_change_handler: Arc<Mutex<Option<OnSignalingChangeHandler>>>,
    on_add_stream_handler: Arc<Mutex<Option<OnAddStreamHandler>>>,
    on_remove_stream_handler: Arc<Mutex<Option<OnRemoveStreamHandler>>>,
    on_data_channel_handler: Arc<Mutex<Option<OnDataChannelHandler>>>,
    on_renegotiation_needed_handler: Arc<Mutex<Option<OnRenegotiationNeededHandler>>>,
    on_negotiation_needed_event_handler: Arc<Mutex<Option<OnNegotiationNeededEventHandler>>>,
    on_ice_connection_change_handler: Arc<Mutex<Option<OnIceConnectionChangeHandler>>>,
    on_standardized_ice_connection_change_handler:
        Arc<Mutex<Option<OnStandardizedIceConnectionChangeHandler>>>,
    on_connection_change_handler: Arc<Mutex<Option<OnConnectionChangeHandler>>>,
    on_ice_gathering_change_handler: Arc<Mutex<Option<OnIceGatheringChangeHandler>>>,
    on_ice_candidate_handler: Arc<Mutex<Option<OnIceCandidateHandler>>>,
    on_ice_candidate_error_handler: Arc<Mutex<Option<OnIceCandidateErrorHandler>>>,
    on_ice_candidates_removed_handler: Arc<Mutex<Option<OnIceCandidatesRemovedHandler>>>,
    on_ice_connection_receiving_change_handler:
        Arc<Mutex<Option<OnIceConnectionReceivingChangeHandler>>>,
    on_ice_selected_candidate_pair_changed_handler:
        Arc<Mutex<Option<OnIceSelectedCandidatePairChangedHandler>>>,
    on_add_track_handler: Arc<Mutex<Option<OnAddTrackHandler>>>,
    on_track_handler: Arc<Mutex<Option<OnTrackHandler>>>,
    on_remove_track_handler: Arc<Mutex<Option<OnRemoveTrackHandler>>>,
    on_interesting_usage_handler: Arc<Mutex<Option<OnInterestingUsageHandler>>>,
}

impl Default for InternalObserver {
    fn default() -> Self {
        Self {
            on_signaling_change_handler: Arc::new(Default::default()),
            on_add_stream_handler: Arc::new(Default::default()),
            on_remove_stream_handler: Arc::new(Default::default()),
            on_data_channel_handler: Arc::new(Default::default()),
            on_renegotiation_needed_handler: Arc::new(Default::default()),
            on_negotiation_needed_event_handler: Arc::new(Default::default()),
            on_ice_connection_change_handler: Arc::new(Default::default()),
            on_standardized_ice_connection_change_handler: Arc::new(Default::default()),
            on_connection_change_handler: Arc::new(Default::default()),
            on_ice_gathering_change_handler: Arc::new(Default::default()),
            on_ice_candidate_handler: Arc::new(Default::default()),
            on_ice_candidate_error_handler: Arc::new(Default::default()),
            on_ice_candidates_removed_handler: Arc::new(Default::default()),
            on_ice_connection_receiving_change_handler: Arc::new(Default::default()),
            on_ice_selected_candidate_pair_changed_handler: Arc::new(Default::default()),
            on_add_track_handler: Arc::new(Default::default()),
            on_track_handler: Arc::new(Default::default()),
            on_remove_track_handler: Arc::new(Default::default()),
            on_interesting_usage_handler: Arc::new(Default::default()),
        }
    }
}

// Observers are being called on the Signaling Thread
impl sys_pc::PeerConnectionObserver for InternalObserver {
    fn on_signaling_change(&self, new_state: SignalingState) {
        trace!("on_signaling_change, {:?}", new_state);
        let mut handler = self.on_signaling_change_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            f(new_state);
        }
    }

    fn on_add_stream(
        &self,
        stream: UniquePtr<libwebrtc_sys::media_stream_interface::ffi::MediaStreamInterface>,
    ) {
        trace!("on_add_stream");
        let mut handler = self.on_add_stream_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            // TODO(theomonnom)
        }
    }

    fn on_remove_stream(
        &self,
        stream: UniquePtr<libwebrtc_sys::media_stream_interface::ffi::MediaStreamInterface>,
    ) {
        trace!("on_remove_stream");
        let mut handler = self.on_remove_stream_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            // TODO(theomonnom)
        }
    }

    fn on_data_channel(
        &self,
        data_channel: UniquePtr<libwebrtc_sys::data_channel::ffi::DataChannel>,
    ) {
        trace!("on_data_channel");
        let mut handler = self.on_data_channel_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            // TODO(theomonnom)
        }
    }

    fn on_renegotiation_needed(&self) {
        trace!("on_renegotiation_needed");
        let mut handler = self.on_renegotiation_needed_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            f();
        }
    }

    fn on_negotiation_needed_event(&self, event: u32) {
        trace!("on_negotiation_needed_event");
        let mut handler = self.on_negotiation_needed_event_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            f(event);
        }
    }

    fn on_ice_connection_change(&self, new_state: IceConnectionState) {
        trace!("on_ice_connection_change");
        let mut handler = self.on_ice_connection_change_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            f(new_state);
        }
    }

    fn on_standardized_ice_connection_change(&self, new_state: IceConnectionState) {
        trace!("on_standardized_ice_connection_change");
        let mut handler = self
            .on_standardized_ice_connection_change_handler
            .lock()
            .unwrap();
        if let Some(f) = handler.as_mut() {
            f(new_state);
        }
    }

    fn on_connection_change(&self, new_state: PeerConnectionState) {
        trace!("on_connection_change");
        let mut handler = self.on_connection_change_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            f(new_state);
        }
    }

    fn on_ice_gathering_change(&self, new_state: IceGatheringState) {
        trace!("on_ice_gathering_change");
        let mut handler = self.on_ice_gathering_change_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            f(new_state);
        }
    }

    fn on_ice_candidate(&self, candidate: UniquePtr<libwebrtc_sys::jsep::ffi::IceCandidate>) {
        trace!("on_ice_candidate");
        let mut handler = self.on_ice_candidate_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            // TODO(theomonnom)
        }
    }

    fn on_ice_candidate_error(
        &self,
        address: String,
        port: i32,
        url: String,
        error_code: i32,
        error_text: String,
    ) {
        trace!("on_ice_candidate_error");
        let mut handler = self.on_ice_candidate_error_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            f(address, port, url, error_code, error_text);
        }
    }

    fn on_ice_candidates_removed(
        &self,
        removed: Vec<UniquePtr<libwebrtc_sys::candidate::ffi::Candidate>>,
    ) {
        trace!("on_ice_candidates_removed");
        let mut handler = self.on_ice_candidates_removed_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            // TODO(theomonnom)
        }
    }

    fn on_ice_connection_receiving_change(&self, receiving: bool) {
        trace!("on_ice_connection_receiving_change");
        let mut handler = self
            .on_ice_connection_receiving_change_handler
            .lock()
            .unwrap();
        if let Some(f) = handler.as_mut() {
            f(receiving);
        }
    }

    fn on_ice_selected_candidate_pair_changed(
        &self,
        event: libwebrtc_sys::peer_connection::ffi::CandidatePairChangeEvent,
    ) {
        trace!("on_ice_selected_candidate_pair_changed");
        let mut handler = self
            .on_ice_selected_candidate_pair_changed_handler
            .lock()
            .unwrap();
        if let Some(f) = handler.as_mut() {
            f(event);
        }
    }

    fn on_add_track(
        &self,
        receiver: UniquePtr<libwebrtc_sys::rtp_receiver::ffi::RtpReceiver>,
        streams: Vec<UniquePtr<libwebrtc_sys::media_stream_interface::ffi::MediaStreamInterface>>,
    ) {
        trace!("on_add_track");
        let mut handler = self.on_add_track_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            // TODO(theomonnom)
        }
    }

    fn on_track(
        &self,
        transceiver: UniquePtr<libwebrtc_sys::rtp_transceiver::ffi::RtpTransceiver>,
    ) {
        trace!("on_track");
        let mut handler = self.on_track_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            // TODO(theomonnom)
        }
    }

    fn on_remove_track(
        &self,
        receiver: UniquePtr<libwebrtc_sys::rtp_receiver::ffi::RtpReceiver>,
    ) {
        trace!("on_remove_track");
        let mut handler = self.on_remove_track_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            // TODO(theomonnom)
        }
    }

    fn on_interesting_usage(&self, usage_pattern: i32) {
        trace!("on_interesting_usage");
        let mut handler = self.on_interesting_usage_handler.lock().unwrap();
        if let Some(f) = handler.as_mut() {
            f(usage_pattern);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::peer_connection_factory::PeerConnectionFactory;
    use libwebrtc_sys::peer_connection_factory::ffi::RTCConfiguration;

    fn init_log() {
        let _ = env_logger::builder().is_test(true).try_init();
    }

    #[tokio::test]
    async fn create_pc() {
        init_log();

        let factory = PeerConnectionFactory::new();
        let config = RTCConfiguration {
            ice_servers: vec![],
        };

        let mut bob = factory.create_peer_connection(config.clone()).unwrap();
        let mut alice = factory.create_peer_connection(config.clone()).unwrap();

        let offer = bob.create_offer().await.unwrap();
        bob.set_local_description(offer.clone()).await.unwrap();
        alice.set_remote_description(offer).await.unwrap();
        let answer = alice.create_answer().await.unwrap();
        alice.set_local_description(answer.clone()).await.unwrap();
        bob.set_remote_description(answer).await.unwrap();

        alice.close();
        bob.close();
    }
}
