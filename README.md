export PKG_CONFIG_PATH=/path/to/libseat/pkgconfig:$PKG_CONFIG_PATH


export XDG_RUNTIME_DIR=/run/user/$(id -u)
The God: https://www.reddit.com/r/pop_os/comments/18hbbf7/comment/kd5uqyw/

RTP test
GST_DEBUG=3 gst-launch-1.0 udpsrc port=6000 ! application/x-rtp, media=video, encoding-name=H264, payload=96 ! rtph264depay ! avdec_h264 ! videoconvert ! autovideosink