/* Compile against the staged FFmpeg public headers to independently verify the Rust prefixes. */
#include <stddef.h>
#include <stdio.h>
#include <libavformat/avformat.h>
#include <libavutil/frame.h>
#include <libavcodec/packet.h>
int main(void) {
#define FIELD(type, field) printf(#type "." #field "=%zu\n", offsetof(type, field))
    FIELD(AVFormatContext, pb); FIELD(AVFormatContext, streams); FIELD(AVFormatContext, duration); FIELD(AVFormatContext, interrupt_callback);
    FIELD(AVStream, codecpar); FIELD(AVStream, time_base);
    FIELD(AVFrame, width); FIELD(AVFrame, color_trc); FIELD(AVFrame, best_effort_timestamp);
    FIELD(AVCodecParameters, coded_side_data); FIELD(AVCodecParameters, nb_coded_side_data);
    FIELD(AVPacket, stream_index);
    return 0;
}