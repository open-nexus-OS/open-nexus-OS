/* Volatile capture hook for the first-quad reference: records VM bindings and,
 * on every SUBMIT_JOBS, writes each job's command stream and every live bound
 * buffer (from the shim's memfd) with its GPU address into PVR_SHIM_DUMP_DIR. */
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

struct dump_map { uint32_t handle; uint64_t va, offset, size; bool live; };
static struct dump_map dump_maps[8192];
static unsigned dump_map_count, dump_job_seq, dump_ctx_seq, dump_fl_seq, dump_hwrt_seq;

static const char *dump_dir(void) { return getenv("PVR_SHIM_DUMP_DIR"); }

static FILE *dump_open(const char *name, const char *mode)
{
   char path[1024];
   if (!dump_dir()) return NULL;
   snprintf(path, sizeof(path), "%s/%s", dump_dir(), name);
   return fopen(path, mode);
}

static void dump_log(const char *fmt, ...)
{
   FILE *f = dump_open("log.txt", "a");
   if (!f) return;
   va_list ap; va_start(ap, fmt); vfprintf(f, fmt, ap); va_end(ap);
   fclose(f);
}

static int dump_ioctl_vm_map(UNUSED int fd, UNUSED unsigned long request, void *arg)
{
   struct drm_pvr_ioctl_vm_map_args *a = arg;
   if (dump_map_count < ARRAY_SIZE(dump_maps))
      dump_maps[dump_map_count++] = (struct dump_map){ a->handle, a->device_addr, a->offset, a->size, true };
   dump_log("map handle=%u va=0x%llx offset=0x%llx size=0x%llx flags=0x%x\n", a->handle,
            (unsigned long long)a->device_addr, (unsigned long long)a->offset,
            (unsigned long long)a->size, a->flags);
   return 0;
}

static int dump_ioctl_vm_unmap(UNUSED int fd, UNUSED unsigned long request, void *arg)
{
   struct drm_pvr_ioctl_vm_unmap_args *a = arg;
   for (unsigned i = 0; i < dump_map_count; i++)
      if (dump_maps[i].live && dump_maps[i].va == a->device_addr) dump_maps[i].live = false;
   dump_log("unmap va=0x%llx size=0x%llx\n", (unsigned long long)a->device_addr, (unsigned long long)a->size);
   return 0;
}

static int dump_ioctl_create_context(UNUSED int fd, UNUSED unsigned long request, void *arg)
{
   struct drm_pvr_ioctl_create_context_args *a = arg;
   a->handle = ++dump_ctx_seq;
   dump_log("context handle=%u type=%u priority=%d flags=0x%x state_len=%u callstack=0x%llx\n",
            a->handle, a->type, a->priority, a->flags, a->static_context_state_len,
            (unsigned long long)a->callstack_addr);
   if (a->static_context_state_len) {
      char name[64]; snprintf(name, sizeof(name), "context%u.state", a->handle);
      FILE *f = dump_open(name, "wb");
      if (f) { fwrite((void *)(uintptr_t)a->static_context_state, 1, a->static_context_state_len, f); fclose(f); }
   }
   return 0;
}

static int dump_ioctl_create_free_list(UNUSED int fd, UNUSED unsigned long request, void *arg)
{
   struct drm_pvr_ioctl_create_free_list_args *a = arg;
   a->handle = ++dump_fl_seq;
   dump_log("freelist handle=%u gpu_addr=0x%llx initial=%u max=%u grow=%u threshold=%u\n", a->handle,
            (unsigned long long)a->free_list_gpu_addr, a->initial_num_pages, a->max_num_pages,
            a->grow_num_pages, a->grow_threshold);
   return 0;
}

static int dump_ioctl_create_hwrt(UNUSED int fd, UNUSED unsigned long request, void *arg)
{
   struct drm_pvr_ioctl_create_hwrt_dataset_args *a = arg;
   a->handle = ++dump_hwrt_seq;
   char name[64]; snprintf(name, sizeof(name), "hwrt%u.args", a->handle);
   FILE *f = dump_open(name, "wb");
   if (f) { fwrite(a, 1, sizeof(*a), f); fclose(f); }
   dump_log("hwrt handle=%u %ux%u samples=%u layers=%u\n", a->handle, a->width, a->height, a->samples, a->layers);
   return 0;
}

static int dump_ioctl_submit_jobs(int fd, UNUSED unsigned long request, void *arg)
{
   struct drm_pvr_ioctl_submit_jobs_args *a = arg;
   struct shim_fd *shim_fd = drm_shim_fd_lookup(fd);
   for (uint32_t i = 0; i < a->jobs.count; i++) {
      struct drm_pvr_job *job = (void *)(uintptr_t)(a->jobs.array + (uint64_t)i * a->jobs.stride);
      unsigned n = dump_job_seq++;
      char name[64];
      snprintf(name, sizeof(name), "job%u.stream", n);
      FILE *f = dump_open(name, "wb");
      if (f) { fwrite((void *)(uintptr_t)job->cmd_stream, 1, job->cmd_stream_len, f); fclose(f); }
      snprintf(name, sizeof(name), "job%u.txt", n);
      FILE *t = dump_open(name, "w");
      if (!t) continue;
      fprintf(t, "type=%u context=%u flags=0x%x stream_len=%u syncs=%u hwrt=%u/%u\n", job->type,
              job->context_handle, job->flags, job->cmd_stream_len, job->sync_ops.count,
              job->hwrt.set_handle, job->hwrt.data_index);
      for (unsigned m = 0; m < dump_map_count; m++) {
         struct dump_map *d = &dump_maps[m];
         if (!d->live) continue;
         struct shim_bo *bo = drm_shim_bo_lookup(shim_fd, d->handle);
         if (!bo) { fprintf(t, "bo va=0x%llx size=0x%llx handle=%u MISSING\n", (unsigned long long)d->va, (unsigned long long)d->size, d->handle); continue; }
         snprintf(name, sizeof(name), "job%u-bo-%012llx.bin", n, (unsigned long long)d->va);
         FILE *b = dump_open(name, "wb");
         if (b) {
            uint8_t buf[65536]; uint64_t done = 0;
            while (done < d->size) {
               size_t chunk = d->size - done > sizeof(buf) ? sizeof(buf) : (size_t)(d->size - done);
               ssize_t r = pread(shim_device.mem_fd, buf, chunk, bo->mem_addr + d->offset + done);
               if (r <= 0) break;
               fwrite(buf, 1, (size_t)r, b); done += (uint64_t)r;
            }
            fclose(b);
         }
         fprintf(t, "bo va=0x%llx size=0x%llx handle=%u bo_size=%u\n", (unsigned long long)d->va,
                 (unsigned long long)d->size, d->handle, bo->size);
         drm_shim_bo_put(bo);
      }
      fclose(t);
      dump_log("submit job%u type=%u\n", n, job->type);
   }
   return 0;
}
