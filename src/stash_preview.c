/* Compiled against the bundled FFmpeg headers. No firmware ABI offsets or DT_NEEDED. */
#include "stash_preview.h"
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libavutil/pixdesc.h>
#include <libswscale/swscale.h>
#define API_LIST(X) \
 X(avformat_alloc_context) X(avio_alloc_context) X(av_malloc) X(av_free) \
 X(avformat_open_input) X(avformat_find_stream_info) X(av_find_best_stream) \
 X(avcodec_alloc_context3) X(avcodec_parameters_to_context) X(avcodec_open2) \
 X(av_packet_alloc) X(av_frame_alloc) X(av_read_frame) X(avcodec_send_packet) \
 X(avcodec_receive_frame) X(av_packet_unref) X(av_seek_frame) X(avcodec_flush_buffers) \
 X(sws_getContext) X(sws_scale) X(sws_freeContext) X(av_frame_free) \
 X(av_packet_free) X(avcodec_free_context) X(avformat_close_input) X(avio_context_free) \
 X(av_dict_set) X(av_dict_free) X(av_pix_fmt_desc_get)
struct preview {
 void *libs[4]; AVFormatContext *fmt; AVIOContext *io; AVCodecContext *codec;
 AVPacket *packet; AVFrame *frame; struct SwsContext *scale; int stream, sw, sh, sf, draining;
#define FIELD(n) __typeof__(&n) n;
 API_LIST(FIELD)
#undef FIELD
};
/* The reservation assumes 8-bit software frames. Reject hardware/unknown/high-depth formats
 * rather than treating a 10/12-bit reference frame as an 8-bit allocation. */
static int preview_format(struct preview *p, int format) {
 const AVPixFmtDescriptor *desc=p->av_pix_fmt_desc_get(format);
 if(!desc || desc->nb_components<1 || desc->flags&AV_PIX_FMT_FLAG_HWACCEL) return 0;
 /* Chroma 4:4:4 / RGB references exceed the 4:2:0 DPB reservation even at eight bits. */
 if(desc->nb_components!=1 && (desc->nb_components!=3 || desc->log2_chroma_w!=1 || desc->log2_chroma_h!=1)) return 0;
 for(int i=0;i<desc->nb_components;i++) if(desc->comp[i].depth>8) return 0;
 return 1;
}
void stash_preview_close(void *decoder) {
 struct preview *p=decoder; if (!p) return;
 if (p->scale && p->sws_freeContext) p->sws_freeContext(p->scale);
 if (p->frame && p->av_frame_free) p->av_frame_free(&p->frame);
 if (p->packet && p->av_packet_free) p->av_packet_free(&p->packet);
 if (p->codec && p->avcodec_free_context) p->avcodec_free_context(&p->codec);
 if (p->fmt && p->avformat_close_input) p->avformat_close_input(&p->fmt);
 if (p->io && p->avio_context_free) { p->av_free(p->io->buffer); p->io->buffer=NULL; p->avio_context_free(&p->io); }
 for (int i=3;i>=0;i--) if (p->libs[i]) dlclose(p->libs[i]);
 free(p);
}
void *stash_preview_open(const char *dir, void *source, stash_read_fn read, stash_seek_fn seek) {
 struct preview *p=calloc(1,sizeof(*p)); if (!p) return NULL;
#ifdef __APPLE__
 const char *names[]={"libavutil-plx.61.dylib","libavcodec-plx.63.dylib","libavformat-plx.63.dylib","libswscale-plx.10.dylib"};
#else
 const char *names[]={"libavutil-plx.so.61","libavcodec-plx.so.63","libavformat-plx.so.63","libswscale-plx.so.10"};
#endif
 for(int i=0;i<4;i++){ char path[4096]; if(snprintf(path,sizeof(path),"%s/%s",dir,names[i])>=(int)sizeof(path)) goto fail;
  p->libs[i]=dlopen(path,RTLD_NOW|RTLD_GLOBAL); if(!p->libs[i]) goto fail; }
#define LOAD(n) do { for(int i=0;i<4&&!p->n;i++) *(void **)(&p->n)=dlsym(p->libs[i],#n); if(!p->n) goto fail; } while(0);
 API_LIST(LOAD)
#undef LOAD
 p->fmt=p->avformat_alloc_context(); if(!p->fmt) goto fail;
 uint8_t *buf=p->av_malloc(32768); if(!buf) goto fail;
 p->io=p->avio_alloc_context(buf,32768,0,source,read,NULL,seek); if(!p->io){p->av_free(buf);goto fail;}
 p->fmt->pb=p->io; p->fmt->flags|=AVFMT_FLAG_CUSTOM_IO;
 /* Cap probing and decoder threading on the 32-bit TV. */
 p->fmt->probesize=1024*1024; p->fmt->max_analyze_duration=3000000;p->fmt->max_streams=32;
 if(p->avformat_open_input(&p->fmt,NULL,NULL,NULL)<0) goto fail;
 unsigned count=p->fmt->nb_streams;if(count<1||count>32) goto fail;
 /* Do not let probing discover an extra stream without the bounded decoder options below. */
 p->fmt->max_streams=(int)count;
 /* Container-declared raster limits apply before probing can open a temporary decoder. */
 for(unsigned i=0;i<count;i++) {
  AVCodecParameters *par=p->fmt->streams[i]->codecpar;
  if(par->codec_type!=AVMEDIA_TYPE_VIDEO) continue;
  if(par->width>1280||par->height>720||par->bits_per_raw_sample>8) goto fail;
  if(par->format>=0&&!preview_format(p,par->format)) goto fail;
 }
 AVDictionary **options=calloc(count?count:1,sizeof(*options));if(!options) goto fail;
 int configured=1;
 for(unsigned i=0;i<count;i++) {
  if(p->av_dict_set(&options[i],"threads","1",0)<0 ||
     p->av_dict_set(&options[i],"max_pixels","921600",0)<0 ||
     p->av_dict_set(&options[i],"skip_frame","nokey",0)<0) configured=0;
 }
 int info=configured?p->avformat_find_stream_info(p->fmt,options):-1;
 for(unsigned i=0;i<count;i++) {
  p->av_dict_free(&options[i]);
 }
 free(options);
 if(info<0) goto fail;
 const AVCodec *codec=NULL; p->stream=p->av_find_best_stream(p->fmt,AVMEDIA_TYPE_VIDEO,-1,-1,&codec,0);
 if(p->stream<0 || !codec) goto fail;
 AVCodecParameters *par=p->fmt->streams[p->stream]->codecpar;
 if(par->width<1||par->height<1||par->width>1280||par->height>720||!preview_format(p,par->format)) goto fail;
 p->codec=p->avcodec_alloc_context3(codec); if(!p->codec) goto fail;
 p->codec->thread_count=1;
 p->codec->max_pixels=1280*720;
 if(p->avcodec_parameters_to_context(p->codec,par)<0||p->avcodec_open2(p->codec,codec,NULL)<0) goto fail;
 p->packet=p->av_packet_alloc(); p->frame=p->av_frame_alloc(); if(!p->packet||!p->frame) goto fail;
 return p;
fail: stash_preview_close(p); return NULL;
}
int stash_preview_next(void *decoder,uint8_t *rgba,int *width,int *height,int64_t *pts_ms) {
 struct preview *p=decoder;
 for(;;){
  int r=p->avcodec_receive_frame(p->codec,p->frame);
  if(r>=0) break;
  if(r!=AVERROR(EAGAIN)&&r!=AVERROR_EOF) return -1;
  if(p->draining){
   if(r!=AVERROR_EOF||p->av_seek_frame(p->fmt,p->stream,0,AVSEEK_FLAG_BACKWARD)<0) return -1;
   p->avcodec_flush_buffers(p->codec);p->draining=0;return 0;
  }
  if(p->av_read_frame(p->fmt,p->packet)<0){
   if(p->avcodec_send_packet(p->codec,NULL)<0) return -1;
   p->draining=1;continue;
  }
  int send=0; if(p->packet->stream_index==p->stream) send=p->avcodec_send_packet(p->codec,p->packet);
  p->av_packet_unref(p->packet); if(send<0&&send!=AVERROR(EAGAIN)) return -1;
 }
 AVFrame *f=p->frame; if(f->width<1||f->height<1||f->width>1280||f->height>720||!preview_format(p,f->format)) return -1;
 double ratio=640.0/f->width; if(360.0/f->height<ratio) ratio=360.0/f->height; if(ratio>1) ratio=1;
 *width=(int)(f->width*ratio); *height=(int)(f->height*ratio);
 if(!p->scale||p->sw!=f->width||p->sh!=f->height||p->sf!=f->format){
  if(p->scale) p->sws_freeContext(p->scale);
  p->scale=p->sws_getContext(f->width,f->height,f->format,*width,*height,AV_PIX_FMT_RGBA,SWS_FAST_BILINEAR,NULL,NULL,NULL);
  p->sw=f->width;p->sh=f->height;p->sf=f->format;
 }
 if(!p->scale) return -1;
 uint8_t *out[]={rgba,NULL,NULL,NULL}; int stride[]={*width*4,0,0,0};
 if(p->sws_scale(p->scale,(const uint8_t *const *)f->data,f->linesize,0,f->height,out,stride)<0) return -1;
 *pts_ms=f->best_effort_timestamp==AV_NOPTS_VALUE?0:(int64_t)(f->best_effort_timestamp*av_q2d(p->fmt->streams[p->stream]->time_base)*1000);
 return 1;
}
