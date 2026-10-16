/*
 * Encoder-only throughput benchmark.
 *
 * Preloads K frames (into GPU memory for hardware encoders), then runs M
 * encoder instances on M threads, each encoding N frames by cycling through
 * the preloaded set. Reports per-instance and aggregate fps.
 *
 * usage: m2c_bench <input> <encoder> <K> <N> <M> [key=value encoder options...]
 */
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <inttypes.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/hwcontext.h>
#include <libavutil/opt.h>

typedef struct Job {
    const char *encoder;
    AVFrame   **frames;
    int         nb_frames, total;
    AVBufferRef *frames_ref;
    int          width, height;
    AVDictionary *opts;
    double       seconds;
    int64_t      bytes;
    int          error;
    struct Job  *self;
} Job;

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec * 1e-9;
}

static void *run(void *arg)
{
    Job *j = arg;
    const AVCodec *codec = avcodec_find_encoder_by_name(j->encoder);
    AVCodecContext *c = avcodec_alloc_context3(codec);
    AVPacket *pkt = av_packet_alloc();
    AVDictionary *opts = NULL;
    int ret;

    c->width     = j->width;
    c->height    = j->height;
    c->time_base = (AVRational){ 1001, 30000 };
    c->framerate = (AVRational){ 30000, 1001 };
    c->sample_aspect_ratio = (AVRational){ 1, 1 };
    if (j->frames_ref) {
        c->pix_fmt       = AV_PIX_FMT_CUDA;
        c->hw_frames_ctx = av_buffer_ref(j->frames_ref);
    } else {
        c->pix_fmt = AV_PIX_FMT_YUV420P;
    }
    av_dict_copy(&opts, j->opts, 0);
    if ((ret = avcodec_open2(c, codec, &opts)) < 0) {
        fprintf(stderr, "open failed: %s\n", av_err2str(ret));
        j->error = ret;
        return NULL;
    }
    if (av_dict_count(opts)) {
        const AVDictionaryEntry *e = NULL;
        while ((e = av_dict_iterate(opts, e)))
            fprintf(stderr, "warning: option %s not used\n", e->key);
    }
    av_dict_free(&opts);
    if (j == j->self)
        fprintf(stderr, "%s: max_b_frames=%d threads=%d bit_rate=%"PRId64"\n",
                j->encoder, c->max_b_frames, c->thread_count, c->bit_rate);

    double t0 = now();
    for (int i = 0; i <= j->total; i++) {
        AVFrame *f = NULL;
        if (i < j->total) {
            f = j->frames[i % j->nb_frames];
            f->pts = i;
        }
        /* frames are shared between threads: pass a reference */
        AVFrame *ref = NULL;
        if (f) {
            ref = av_frame_clone(f);
            ref->pts = i;
            /* decoders tag every frame as intra; don't force I pictures */
            ref->pict_type = AV_PICTURE_TYPE_NONE;
            ref->flags &= ~AV_FRAME_FLAG_KEY;
        }
        ret = avcodec_send_frame(c, ref);
        av_frame_free(&ref);
        if (ret < 0) {
            j->error = ret;
            break;
        }
        while ((ret = avcodec_receive_packet(c, pkt)) >= 0) {
            j->bytes += pkt->size;
            av_packet_unref(pkt);
        }
    }
    j->seconds = now() - t0;
    av_packet_free(&pkt);
    avcodec_free_context(&c);
    return NULL;
}

int main(int argc, char **argv)
{
    if (argc < 6) {
        fprintf(stderr, "usage: %s <input> <encoder> <K> <N> <M> [opt=val...]\n", argv[0]);
        return 1;
    }
    const char *input = argv[1], *encoder = argv[2];
    int K = atoi(argv[3]), N = atoi(argv[4]), M = atoi(argv[5]);
    AVDictionary *opts = NULL;
    for (int i = 6; i < argc; i++) {
        char *eq = strchr(argv[i], '=');
        if (!eq)
            continue;
        *eq = 0;
        av_dict_set(&opts, argv[i], eq + 1, 0);
    }

    /* decode K frames */
    AVFormatContext *fmt = NULL;
    if (avformat_open_input(&fmt, input, NULL, NULL) < 0 || avformat_find_stream_info(fmt, NULL) < 0)
        return 1;
    int si = av_find_best_stream(fmt, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    AVStream *st = fmt->streams[si];
    const AVCodec *dc = avcodec_find_decoder(st->codecpar->codec_id);
    AVCodecContext *dec = avcodec_alloc_context3(dc);
    avcodec_parameters_to_context(dec, st->codecpar);
    avcodec_open2(dec, dc, NULL);

    AVFrame **frames = calloc(K, sizeof(*frames));
    AVPacket *pkt = av_packet_alloc();
    int n = 0;
    while (n < K && av_read_frame(fmt, pkt) >= 0) {
        if (pkt->stream_index == si && avcodec_send_packet(dec, pkt) >= 0) {
            AVFrame *f = av_frame_alloc();
            while (n < K && avcodec_receive_frame(dec, f) >= 0) {
                frames[n++] = f;
                f = av_frame_alloc();
            }
            av_frame_free(&f);
        }
        av_packet_unref(pkt);
    }
    K = n;
    int w = frames[0]->width, h = frames[0]->height;

    /* upload to GPU if the encoder is a CUDA one */
    AVBufferRef *dev = NULL, *frames_ref = NULL;
    if (strstr(encoder, "cuda") || strstr(encoder, "nvenc")) {
        if (av_hwdevice_ctx_create(&dev, AV_HWDEVICE_TYPE_CUDA, NULL, NULL, 0) < 0)
            return 1;
        frames_ref = av_hwframe_ctx_alloc(dev);
        AVHWFramesContext *fc = (AVHWFramesContext *)frames_ref->data;
        fc->format    = AV_PIX_FMT_CUDA;
        fc->sw_format = AV_PIX_FMT_YUV420P;
        fc->width     = w;
        fc->height    = h;
        if (av_hwframe_ctx_init(frames_ref) < 0)
            return 1;
        for (int i = 0; i < K; i++) {
            AVFrame *g = av_frame_alloc();
            av_hwframe_get_buffer(frames_ref, g, 0);
            av_hwframe_transfer_data(g, frames[i], 0);
            av_frame_free(&frames[i]);
            frames[i] = g;
        }
    }

    Job *jobs = calloc(M, sizeof(*jobs));
    pthread_t *th = calloc(M, sizeof(*th));
    double t0 = now();
    for (int i = 0; i < M; i++) {
        jobs[i] = (Job){ .encoder = encoder, .frames = frames, .nb_frames = K, .total = N,
                         .frames_ref = frames_ref, .width = w, .height = h, .opts = opts };
        jobs[i].self = i == 0 ? &jobs[i] : NULL;
        pthread_create(&th[i], NULL, run, &jobs[i]);
    }
    double minfps = 1e9;
    int64_t bytes = 0;
    for (int i = 0; i < M; i++) {
        pthread_join(th[i], NULL);
        if (jobs[i].error) {
            fprintf(stderr, "instance %d failed: %s\n", i, av_err2str(jobs[i].error));
            return 1;
        }
        double fps = N / jobs[i].seconds;
        if (fps < minfps)
            minfps = fps;
        bytes += jobs[i].bytes;
    }
    double wall = now() - t0;
    printf("%s K=%d N=%d M=%d: aggregate %.1f fps, slowest instance %.1f fps, avg %.2f Mbps\n",
           encoder, K, N, M, (double)N * M / wall, minfps,
           bytes * 8.0 / M / (N / 29.97) / 1e6);
    return 0;
}
