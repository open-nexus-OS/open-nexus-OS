/* Reference capture for Block 4 layer 4: a clear and a filtered, scaled blit of a
 * linear 256x256 RGBA8 test texture into a linear 1920x1080 BGRA8 target, through
 * the open Vulkan driver over the DRM shim. The shim's hook writes the command
 * streams and every bound buffer with its GPU address into PVR_SHIM_DUMP_DIR. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vulkan/vulkan.h>

#define CK(x) do { VkResult r_ = (x); if (r_ != VK_SUCCESS) { fprintf(stderr, "%s -> %d\n", #x, r_); exit(1); } } while (0)

static VkPhysicalDevice pd;
static VkDevice dev;

static uint32_t mem_type(uint32_t bits, VkMemoryPropertyFlags want)
{
   VkPhysicalDeviceMemoryProperties mp;
   vkGetPhysicalDeviceMemoryProperties(pd, &mp);
   for (uint32_t i = 0; i < mp.memoryTypeCount; i++)
      if (((bits >> i) & 1) && (mp.memoryTypes[i].propertyFlags & want) == want) return i;
   for (uint32_t i = 0; i < mp.memoryTypeCount; i++)
      if ((bits >> i) & 1) return i;
   fprintf(stderr, "no memory type\n"); exit(1);
}

static void make_image(uint32_t w, uint32_t h, VkFormat fmt, VkImageUsageFlags usage,
                       VkImageLayout initial, VkMemoryPropertyFlags want,
                       VkImage *img, VkDeviceMemory *mem, VkSubresourceLayout *layout, const char *tag)
{
   VkImageCreateInfo ici = { .sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO, .imageType = VK_IMAGE_TYPE_2D,
      .format = fmt, .extent = { w, h, 1 }, .mipLevels = 1, .arrayLayers = 1,
      .samples = VK_SAMPLE_COUNT_1_BIT, .tiling = VK_IMAGE_TILING_LINEAR, .usage = usage,
      .sharingMode = VK_SHARING_MODE_EXCLUSIVE, .initialLayout = initial };
   CK(vkCreateImage(dev, &ici, NULL, img));
   VkMemoryRequirements mr; vkGetImageMemoryRequirements(dev, *img, &mr);
   VkMemoryAllocateInfo mai = { .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, .allocationSize = mr.size,
      .memoryTypeIndex = mem_type(mr.memoryTypeBits, want) };
   CK(vkAllocateMemory(dev, &mai, NULL, mem));
   CK(vkBindImageMemory(dev, *img, *mem, 0));
   VkImageSubresource sub = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 0 };
   vkGetImageSubresourceLayout(dev, *img, &sub, layout);
   printf("%s: %ux%u fmt=%d mem_size=%llu align=%llu rowPitch=%llu offset=%llu size=%llu\n", tag, w, h, fmt,
          (unsigned long long)mr.size, (unsigned long long)mr.alignment, (unsigned long long)layout->rowPitch,
          (unsigned long long)layout->offset, (unsigned long long)layout->size);
}

int main(int argc, char **argv)
{
   const uint32_t W = 1920, H = 1080, TW = 256, TH = 256;
   int32_t dx0 = argc > 1 ? atoi(argv[1]) : 704, dy0 = argc > 2 ? atoi(argv[2]) : 284;
   int32_t dx1 = argc > 3 ? atoi(argv[3]) : 1216, dy1 = argc > 4 ? atoi(argv[4]) : 796;
   const char *out = getenv("PVR_SHIM_DUMP_DIR"); if (!out) out = ".";

   VkApplicationInfo ai = { .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO, .pApplicationName = "first-quad",
      .apiVersion = VK_API_VERSION_1_1 };
   VkInstanceCreateInfo ici = { .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, .pApplicationInfo = &ai };
   VkInstance inst; CK(vkCreateInstance(&ici, NULL, &inst));
   uint32_t n = 1; VkResult er = vkEnumeratePhysicalDevices(inst, &n, &pd);
   if ((er != VK_SUCCESS && er != VK_INCOMPLETE) || n == 0) { fprintf(stderr, "no device (%d, n=%u)\n", er, n); return 1; }
   VkPhysicalDeviceProperties props; vkGetPhysicalDeviceProperties(pd, &props);
   printf("device: %s api=%u.%u driver=0x%x\n", props.deviceName, VK_VERSION_MAJOR(props.apiVersion),
          VK_VERSION_MINOR(props.apiVersion), props.driverVersion);

   float prio = 1.0f;
   VkDeviceQueueCreateInfo qci = { .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO, .queueFamilyIndex = 0,
      .queueCount = 1, .pQueuePriorities = &prio };
   VkDeviceCreateInfo dci = { .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, .queueCreateInfoCount = 1,
      .pQueueCreateInfos = &qci };
   CK(vkCreateDevice(pd, &dci, NULL, &dev));
   VkQueue q; vkGetDeviceQueue(dev, 0, 0, &q);

   VkImage dst, src; VkDeviceMemory dstm, srcm; VkSubresourceLayout dl, sl;
   make_image(W, H, VK_FORMAT_B8G8R8A8_UNORM, VK_IMAGE_USAGE_TRANSFER_DST_BIT, VK_IMAGE_LAYOUT_UNDEFINED,
              VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT, &dst, &dstm, &dl, "dst");
   make_image(TW, TH, VK_FORMAT_R8G8B8A8_UNORM, VK_IMAGE_USAGE_TRANSFER_SRC_BIT, VK_IMAGE_LAYOUT_PREINITIALIZED,
              VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT, &src, &srcm, &sl, "src");

   /* The texture: a 32-px checker of two colours under a diagonal alpha ramp, with a 4-px frame. */
   void *p; CK(vkMapMemory(dev, srcm, 0, VK_WHOLE_SIZE, 0, &p));
   uint8_t *base = (uint8_t *)p + sl.offset;
   char path[1024]; snprintf(path, sizeof(path), "%s/texture.rgba", out); FILE *tf = fopen(path, "wb");
   for (uint32_t y = 0; y < TH; y++) {
      uint8_t *row = base + (size_t)y * sl.rowPitch;
      for (uint32_t x = 0; x < TW; x++) {
         int frame = x < 4 || y < 4 || x >= TW - 4 || y >= TH - 4;
         int check = ((x >> 5) ^ (y >> 5)) & 1;
         uint8_t r = frame ? 255 : (check ? 230 : 40), g = frame ? 255 : (check ? 120 : 90),
                 b = frame ? 255 : (check ? 30 : 200), a = (uint8_t)(((x + y) * 255) / (TW + TH - 2));
         row[x * 4 + 0] = r; row[x * 4 + 1] = g; row[x * 4 + 2] = b; row[x * 4 + 3] = a;
         if (tf) fwrite(row + x * 4, 1, 4, tf);
      }
   }
   if (tf) fclose(tf);
   vkUnmapMemory(dev, srcm);

   VkCommandPoolCreateInfo cpci = { .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO, .queueFamilyIndex = 0 };
   VkCommandPool pool; CK(vkCreateCommandPool(dev, &cpci, NULL, &pool));
   VkCommandBufferAllocateInfo cbai = { .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO, .commandPool = pool,
      .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = 1 };
   VkCommandBuffer cb; CK(vkAllocateCommandBuffers(dev, &cbai, &cb));
   VkCommandBufferBeginInfo bi = { .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
      .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT };
   CK(vkBeginCommandBuffer(cb, &bi));

   VkImageSubresourceRange range = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1 };
   VkImageMemoryBarrier to_dst = { .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER, .dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
      .oldLayout = VK_IMAGE_LAYOUT_UNDEFINED, .newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
      .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .image = dst, .subresourceRange = range };
   VkImageMemoryBarrier to_src = { .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER, .srcAccessMask = VK_ACCESS_HOST_WRITE_BIT,
      .dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT, .oldLayout = VK_IMAGE_LAYOUT_PREINITIALIZED,
      .newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
      .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .image = src, .subresourceRange = range };
   VkImageMemoryBarrier both[2] = { to_dst, to_src };
   vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_HOST_BIT | VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                        VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, NULL, 0, NULL, 2, both);

   VkClearColorValue clear = { .float32 = { 0.10f, 0.12f, 0.16f, 1.0f } };
   vkCmdClearColorImage(cb, dst, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &clear, 1, &range);

   VkImageMemoryBarrier mid = { .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER, .srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
      .dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT, .oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
      .newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
      .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .image = dst, .subresourceRange = range };
   vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, NULL, 0, NULL, 1, &mid);

   VkImageBlit blit = { .srcSubresource = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1 },
      .srcOffsets = { { 0, 0, 0 }, { (int32_t)TW, (int32_t)TH, 1 } },
      .dstSubresource = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1 },
      .dstOffsets = { { dx0, dy0, 0 }, { dx1, dy1, 1 } } };
   vkCmdBlitImage(cb, src, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, dst, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &blit,
                  VK_FILTER_LINEAR);
   CK(vkEndCommandBuffer(cb));

   VkFenceCreateInfo fci = { .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
   VkFence fence; CK(vkCreateFence(dev, &fci, NULL, &fence));
   VkSubmitInfo si = { .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO, .commandBufferCount = 1, .pCommandBuffers = &cb };
   CK(vkQueueSubmit(q, 1, &si, fence));
   VkResult wr = vkWaitForFences(dev, 1, &fence, VK_TRUE, 1000000000ull);
   printf("submitted; fence wait -> %d\n", wr);

   snprintf(path, sizeof(path), "%s/scene.txt", out); FILE *sf = fopen(path, "w");
   if (sf) {
      fprintf(sf, "dst %ux%u B8G8R8A8_UNORM linear rowPitch=%llu offset=%llu\n", W, H, (unsigned long long)dl.rowPitch, (unsigned long long)dl.offset);
      fprintf(sf, "src %ux%u R8G8B8A8_UNORM linear rowPitch=%llu offset=%llu\n", TW, TH, (unsigned long long)sl.rowPitch, (unsigned long long)sl.offset);
      fprintf(sf, "clear 0.10 0.12 0.16 1.0\nblit src 0 0 %u %u -> dst %d %d %d %d filter=linear\n", TW, TH, dx0, dy0, dx1, dy1);
      fclose(sf);
   }
   return 0;
}
