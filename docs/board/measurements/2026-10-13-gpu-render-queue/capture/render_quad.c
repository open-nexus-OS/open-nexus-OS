/* Reference capture for Block 4 layer 5a: a render pass (geometry + fragment jobs) that
 * clears a linear 1920x1080 BGRA8 target and draws N textured quads with source-over
 * blending and a per-quad tint, through the open Vulkan driver over the DRM shim. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vulkan/vulkan.h>

#define CK(x) do { VkResult r_ = (x); if (r_ != VK_SUCCESS) { fprintf(stderr, "%s -> %d\n", #x, r_); exit(1); } } while (0)

static VkPhysicalDevice pd;
static VkDevice dev;
static const char *out = ".";

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

static VkResult make_image(uint32_t w, uint32_t h, VkFormat fmt, VkImageUsageFlags usage, VkImageTiling tiling,
                           VkImageLayout initial, VkMemoryPropertyFlags want, VkImage *img, VkDeviceMemory *mem,
                           VkSubresourceLayout *layout, const char *tag)
{
   VkImageCreateInfo ici = { .sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO, .imageType = VK_IMAGE_TYPE_2D,
      .format = fmt, .extent = { w, h, 1 }, .mipLevels = 1, .arrayLayers = 1, .samples = VK_SAMPLE_COUNT_1_BIT,
      .tiling = tiling, .usage = usage, .sharingMode = VK_SHARING_MODE_EXCLUSIVE, .initialLayout = initial };
   VkResult r = vkCreateImage(dev, &ici, NULL, img);
   if (r != VK_SUCCESS) { printf("%s: %s refused (%d)\n", tag, tiling == VK_IMAGE_TILING_LINEAR ? "linear" : "optimal", r); return r; }
   VkMemoryRequirements mr; vkGetImageMemoryRequirements(dev, *img, &mr);
   VkMemoryAllocateInfo mai = { .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, .allocationSize = mr.size,
      .memoryTypeIndex = mem_type(mr.memoryTypeBits, want) };
   CK(vkAllocateMemory(dev, &mai, NULL, mem));
   CK(vkBindImageMemory(dev, *img, *mem, 0));
   if (tiling == VK_IMAGE_TILING_LINEAR) {
      VkImageSubresource sub = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 0 };
      vkGetImageSubresourceLayout(dev, *img, &sub, layout);
   } else {
      memset(layout, 0, sizeof(*layout));
   }
   printf("%s: %ux%u fmt=%d %s mem_size=%llu rowPitch=%llu\n", tag, w, h, fmt,
          tiling == VK_IMAGE_TILING_LINEAR ? "linear" : "optimal", (unsigned long long)mr.size,
          (unsigned long long)layout->rowPitch);
   return VK_SUCCESS;
}

static VkShaderModule load_shader(const char *path)
{
   FILE *f = fopen(path, "rb"); if (!f) { fprintf(stderr, "no %s\n", path); exit(1); }
   fseek(f, 0, SEEK_END); long n = ftell(f); fseek(f, 0, SEEK_SET);
   uint32_t *code = malloc(n); fread(code, 1, n, f); fclose(f);
   VkShaderModuleCreateInfo ci = { .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO, .codeSize = n, .pCode = code };
   VkShaderModule m; CK(vkCreateShaderModule(dev, &ci, NULL, &m)); return m;
}

static void fill_checker(uint8_t *base, uint32_t pitch, uint32_t TW, uint32_t TH)
{
   for (uint32_t y = 0; y < TH; y++) {
      uint8_t *row = base + (size_t)y * pitch;
      for (uint32_t x = 0; x < TW; x++) {
         int frame = x < 4 || y < 4 || x >= TW - 4 || y >= TH - 4;
         int check = ((x >> 5) ^ (y >> 5)) & 1;
         row[x * 4 + 0] = frame ? 255 : (check ? 230 : 40);
         row[x * 4 + 1] = frame ? 255 : (check ? 120 : 90);
         row[x * 4 + 2] = frame ? 255 : (check ? 30 : 200);
         row[x * 4 + 3] = (uint8_t)(((x + y) * 255) / (TW + TH - 2));
      }
   }
}

int main(int argc, char **argv)
{
   const uint32_t W = 1920, H = 1080, TW = 256, TH = 256;
   int nquads = argc > 1 ? atoi(argv[1]) : 1;
   if (nquads < 1 || nquads > 3) nquads = 1;
   float dx = argc > 2 ? (float)atof(argv[2]) : 0.0f;   /* shifts every quad */
   float tint_r = argc > 3 ? (float)atof(argv[3]) : -1.0f; /* overrides quad 0's red tint */
   int frames = argc > 4 ? atoi(argv[4]) : 1; /* frames submitted in a row */
   /* RQ_AREA="x,y,w,h": the render area (and the pipeline's scissor); RQ_LOAD=1: load, not clear. */
   VkRect2D area = { { 0, 0 }, { W, H } };
   if (getenv("RQ_AREA")) {
      int ax, ay, aw, ah;
      if (sscanf(getenv("RQ_AREA"), "%d,%d,%d,%d", &ax, &ay, &aw, &ah) == 4) {
         area.offset.x = ax; area.offset.y = ay; area.extent.width = (uint32_t)aw; area.extent.height = (uint32_t)ah;
      }
   }
   int load = getenv("RQ_LOAD") && atoi(getenv("RQ_LOAD"));
   if (getenv("PVR_SHIM_DUMP_DIR")) out = getenv("PVR_SHIM_DUMP_DIR");

   VkApplicationInfo ai = { .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO, .pApplicationName = "render-quad", .apiVersion = VK_API_VERSION_1_1 };
   VkInstanceCreateInfo ici = { .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, .pApplicationInfo = &ai };
   VkInstance inst; CK(vkCreateInstance(&ici, NULL, &inst));
   uint32_t n = 1; VkResult er = vkEnumeratePhysicalDevices(inst, &n, &pd);
   if ((er != VK_SUCCESS && er != VK_INCOMPLETE) || n == 0) { fprintf(stderr, "no device\n"); return 1; }
   float prio = 1.0f;
   VkDeviceQueueCreateInfo qci = { .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO, .queueCount = 1, .pQueuePriorities = &prio };
   VkDeviceCreateInfo dci = { .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, .queueCreateInfoCount = 1, .pQueueCreateInfos = &qci };
   CK(vkCreateDevice(pd, &dci, NULL, &dev));
   VkQueue q; vkGetDeviceQueue(dev, 0, 0, &q);

   /* The target: linear first (the plane's layout), optimal as the fallback. */
   VkImage dst; VkDeviceMemory dstm; VkSubresourceLayout dl; int dst_linear = 1;
   if (make_image(W, H, VK_FORMAT_B8G8R8A8_UNORM, VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
                  VK_IMAGE_TILING_LINEAR, VK_IMAGE_LAYOUT_UNDEFINED, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT, &dst, &dstm, &dl, "dst") != VK_SUCCESS) {
      dst_linear = 0;
      CK(make_image(W, H, VK_FORMAT_B8G8R8A8_UNORM, VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
                    VK_IMAGE_TILING_OPTIMAL, VK_IMAGE_LAYOUT_UNDEFINED, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT, &dst, &dstm, &dl, "dst"));
   }
   /* The texture: linear and host-written first; optimal with a staging copy as the fallback. */
   VkImage tex; VkDeviceMemory texm; VkSubresourceLayout tl; int tex_linear = 1;
   VkBuffer staging = VK_NULL_HANDLE; VkDeviceMemory stagingm = VK_NULL_HANDLE;
   if (make_image(TW, TH, VK_FORMAT_R8G8B8A8_UNORM, VK_IMAGE_USAGE_SAMPLED_BIT, VK_IMAGE_TILING_LINEAR,
                  VK_IMAGE_LAYOUT_PREINITIALIZED, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                  &tex, &texm, &tl, "tex") == VK_SUCCESS) {
      void *p; CK(vkMapMemory(dev, texm, 0, VK_WHOLE_SIZE, 0, &p));
      fill_checker((uint8_t *)p + tl.offset, (uint32_t)tl.rowPitch, TW, TH);
      vkUnmapMemory(dev, texm);
   } else {
      tex_linear = 0;
      CK(make_image(TW, TH, VK_FORMAT_R8G8B8A8_UNORM, VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT,
                    VK_IMAGE_TILING_OPTIMAL, VK_IMAGE_LAYOUT_UNDEFINED, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT, &tex, &texm, &tl, "tex"));
      VkBufferCreateInfo bci = { .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO, .size = TW * TH * 4, .usage = VK_BUFFER_USAGE_TRANSFER_SRC_BIT };
      CK(vkCreateBuffer(dev, &bci, NULL, &staging));
      VkMemoryRequirements mr; vkGetBufferMemoryRequirements(dev, staging, &mr);
      VkMemoryAllocateInfo mai = { .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, .allocationSize = mr.size,
         .memoryTypeIndex = mem_type(mr.memoryTypeBits, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT) };
      CK(vkAllocateMemory(dev, &mai, NULL, &stagingm)); CK(vkBindBufferMemory(dev, staging, stagingm, 0));
      void *p; CK(vkMapMemory(dev, stagingm, 0, VK_WHOLE_SIZE, 0, &p)); fill_checker(p, TW * 4, TW, TH); vkUnmapMemory(dev, stagingm);
   }
   VkImageViewCreateInfo tvci = { .sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO, .image = tex, .viewType = VK_IMAGE_VIEW_TYPE_2D,
      .format = VK_FORMAT_R8G8B8A8_UNORM, .subresourceRange = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1 } };
   VkImageView texv; CK(vkCreateImageView(dev, &tvci, NULL, &texv));
   VkSamplerCreateInfo sci = { .sType = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO, .magFilter = VK_FILTER_LINEAR, .minFilter = VK_FILTER_LINEAR,
      .addressModeU = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE, .addressModeV = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
      .addressModeW = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE, .maxLod = 0.0f };
   VkSampler sampler; CK(vkCreateSampler(dev, &sci, NULL, &sampler));

   /* The vertex buffer: N quads of two triangles, pixel positions and uvs. */
   struct { float x, y, u, v; } verts[18];
   float rects[3][2] = { { 704, 284 }, { 304, 134 }, { 1104, 434 } };
   float tints[3][4] = { { 1, 1, 1, 1 }, { 1, 0.5f, 0.5f, 1 }, { 0.5f, 0.5f, 1, 0.6f } };
   for (int i = 0; i < 3; i++) rects[i][0] += dx;
   if (tint_r >= 0.0f) tints[0][0] = tint_r;
   for (int i = 0; i < nquads; i++) {
      float x0 = rects[i][0], y0 = rects[i][1], x1 = x0 + 512, y1 = y0 + 512;
      struct { float x, y, u, v; } quad[6] = { { x0, y0, 0, 0 }, { x1, y0, 1, 0 }, { x1, y1, 1, 1 }, { x0, y0, 0, 0 }, { x1, y1, 1, 1 }, { x0, y1, 0, 1 } };
      memcpy(&verts[i * 6], quad, sizeof(quad));
   }
   VkBufferCreateInfo vbci = { .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO, .size = sizeof(verts), .usage = VK_BUFFER_USAGE_VERTEX_BUFFER_BIT };
   VkBuffer vb; CK(vkCreateBuffer(dev, &vbci, NULL, &vb));
   VkMemoryRequirements vmr; vkGetBufferMemoryRequirements(dev, vb, &vmr);
   VkMemoryAllocateInfo vmai = { .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, .allocationSize = vmr.size,
      .memoryTypeIndex = mem_type(vmr.memoryTypeBits, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT) };
   VkDeviceMemory vbm; CK(vkAllocateMemory(dev, &vmai, NULL, &vbm)); CK(vkBindBufferMemory(dev, vb, vbm, 0));
   { void *p; CK(vkMapMemory(dev, vbm, 0, VK_WHOLE_SIZE, 0, &p)); memcpy(p, verts, sizeof(verts)); vkUnmapMemory(dev, vbm); }

   /* Render pass and framebuffer. */
   VkAttachmentDescription att = { .format = VK_FORMAT_B8G8R8A8_UNORM, .samples = VK_SAMPLE_COUNT_1_BIT,
      .loadOp = load ? VK_ATTACHMENT_LOAD_OP_LOAD : VK_ATTACHMENT_LOAD_OP_CLEAR, .storeOp = VK_ATTACHMENT_STORE_OP_STORE,
      .stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE, .stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE,
      .initialLayout = load ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_UNDEFINED, .finalLayout = VK_IMAGE_LAYOUT_GENERAL };
   VkAttachmentReference ref = { 0, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL };
   VkSubpassDescription sub = { .pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS, .colorAttachmentCount = 1, .pColorAttachments = &ref };
   VkRenderPassCreateInfo rpci = { .sType = VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO, .attachmentCount = 1, .pAttachments = &att, .subpassCount = 1, .pSubpasses = &sub };
   VkRenderPass rp; CK(vkCreateRenderPass(dev, &rpci, NULL, &rp));
   VkImageViewCreateInfo dvci = { .sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO, .image = dst, .viewType = VK_IMAGE_VIEW_TYPE_2D,
      .format = VK_FORMAT_B8G8R8A8_UNORM, .subresourceRange = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1 } };
   VkImageView dstv; CK(vkCreateImageView(dev, &dvci, NULL, &dstv));
   VkFramebufferCreateInfo fbci = { .sType = VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO, .renderPass = rp, .attachmentCount = 1, .pAttachments = &dstv, .width = W, .height = H, .layers = 1 };
   VkFramebuffer fb; CK(vkCreateFramebuffer(dev, &fbci, NULL, &fb));

   /* Descriptors and pipeline. */
   VkDescriptorSetLayoutBinding b0 = { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT };
   VkDescriptorSetLayoutCreateInfo dslci = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO, .bindingCount = 1, .pBindings = &b0 };
   VkDescriptorSetLayout dsl; CK(vkCreateDescriptorSetLayout(dev, &dslci, NULL, &dsl));
   VkPushConstantRange pcr = { VK_SHADER_STAGE_FRAGMENT_BIT, 0, 16 };
   VkPipelineLayoutCreateInfo plci = { .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO, .setLayoutCount = 1, .pSetLayouts = &dsl, .pushConstantRangeCount = 1, .pPushConstantRanges = &pcr };
   VkPipelineLayout pl; CK(vkCreatePipelineLayout(dev, &plci, NULL, &pl));
   VkDescriptorPoolSize ps = { VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 1 };
   VkDescriptorPoolCreateInfo dpci = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO, .maxSets = 1, .poolSizeCount = 1, .pPoolSizes = &ps };
   VkDescriptorPool dp; CK(vkCreateDescriptorPool(dev, &dpci, NULL, &dp));
   VkDescriptorSetAllocateInfo dsai = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO, .descriptorPool = dp, .descriptorSetCount = 1, .pSetLayouts = &dsl };
   VkDescriptorSet ds; CK(vkAllocateDescriptorSets(dev, &dsai, &ds));
   VkDescriptorImageInfo dii = { sampler, texv, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL };
   VkWriteDescriptorSet wds = { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstSet = ds, .dstBinding = 0, .descriptorCount = 1, .descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, .pImageInfo = &dii };
   vkUpdateDescriptorSets(dev, 1, &wds, 0, NULL);

   VkShaderModule vs = load_shader("quad.vert.spv"), fs = load_shader(getenv("RQ_FS") ? getenv("RQ_FS") : "quad.frag.spv");
   int blend = !(getenv("RQ_BLEND") && atoi(getenv("RQ_BLEND")) == 0);
   VkPipelineShaderStageCreateInfo stages[2] = {
      { .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO, .stage = VK_SHADER_STAGE_VERTEX_BIT, .module = vs, .pName = "main" },
      { .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO, .stage = VK_SHADER_STAGE_FRAGMENT_BIT, .module = fs, .pName = "main" } };
   VkVertexInputBindingDescription vbd = { 0, 16, VK_VERTEX_INPUT_RATE_VERTEX };
   VkVertexInputAttributeDescription vad[2] = { { 0, 0, VK_FORMAT_R32G32_SFLOAT, 0 }, { 1, 0, VK_FORMAT_R32G32_SFLOAT, 8 } };
   VkPipelineVertexInputStateCreateInfo vis = { .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO, .vertexBindingDescriptionCount = 1, .pVertexBindingDescriptions = &vbd, .vertexAttributeDescriptionCount = 2, .pVertexAttributeDescriptions = vad };
   VkPipelineInputAssemblyStateCreateInfo ias = { .sType = VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO, .topology = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST };
   VkViewport vp = { 0, 0, (float)W, (float)H, 0, 1 }; VkRect2D sc = area;
   VkPipelineViewportStateCreateInfo vps = { .sType = VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO, .viewportCount = 1, .pViewports = &vp, .scissorCount = 1, .pScissors = &sc };
   VkPipelineRasterizationStateCreateInfo rs = { .sType = VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO, .polygonMode = VK_POLYGON_MODE_FILL, .cullMode = VK_CULL_MODE_NONE, .frontFace = VK_FRONT_FACE_COUNTER_CLOCKWISE, .lineWidth = 1.0f };
   VkPipelineMultisampleStateCreateInfo ms = { .sType = VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO, .rasterizationSamples = VK_SAMPLE_COUNT_1_BIT };
   VkPipelineColorBlendAttachmentState cba = { .blendEnable = blend ? VK_TRUE : VK_FALSE, .srcColorBlendFactor = VK_BLEND_FACTOR_SRC_ALPHA, .dstColorBlendFactor = VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA, .colorBlendOp = VK_BLEND_OP_ADD,
      .srcAlphaBlendFactor = VK_BLEND_FACTOR_ONE, .dstAlphaBlendFactor = VK_BLEND_FACTOR_ZERO, .alphaBlendOp = VK_BLEND_OP_ADD, .colorWriteMask = 0xf };
   VkPipelineColorBlendStateCreateInfo cbs = { .sType = VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO, .attachmentCount = 1, .pAttachments = &cba };
   VkGraphicsPipelineCreateInfo gpci = { .sType = VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO, .stageCount = 2, .pStages = stages, .pVertexInputState = &vis, .pInputAssemblyState = &ias,
      .pViewportState = &vps, .pRasterizationState = &rs, .pMultisampleState = &ms, .pColorBlendState = &cbs, .layout = pl, .renderPass = rp, .subpass = 0 };
   VkPipeline pipe; CK(vkCreateGraphicsPipelines(dev, VK_NULL_HANDLE, 1, &gpci, NULL, &pipe));

   for (int frame = 0; frame < frames; frame++) {
   /* Record. */
   VkCommandPoolCreateInfo cpci = { .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO };
   VkCommandPool pool; CK(vkCreateCommandPool(dev, &cpci, NULL, &pool));
   VkCommandBufferAllocateInfo cbai = { .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO, .commandPool = pool, .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = 1 };
   VkCommandBuffer cb; CK(vkAllocateCommandBuffers(dev, &cbai, &cb));
   VkCommandBufferBeginInfo bi = { .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO, .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT };
   CK(vkBeginCommandBuffer(cb, &bi));
   VkImageSubresourceRange range = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1 };
   if (tex_linear) {
      VkImageMemoryBarrier to_read = { .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER, .srcAccessMask = VK_ACCESS_HOST_WRITE_BIT, .dstAccessMask = VK_ACCESS_SHADER_READ_BIT,
         .oldLayout = VK_IMAGE_LAYOUT_PREINITIALIZED, .newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .image = tex, .subresourceRange = range };
      vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_HOST_BIT, VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT, 0, 0, NULL, 0, NULL, 1, &to_read);
   } else {
      VkImageMemoryBarrier to_dst = { .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER, .dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT, .oldLayout = VK_IMAGE_LAYOUT_UNDEFINED, .newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
         .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .image = tex, .subresourceRange = range };
      vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, NULL, 0, NULL, 1, &to_dst);
      VkBufferImageCopy region = { .imageSubresource = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1 }, .imageExtent = { TW, TH, 1 } };
      vkCmdCopyBufferToImage(cb, staging, tex, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
      VkImageMemoryBarrier to_read = { .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER, .srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT, .dstAccessMask = VK_ACCESS_SHADER_READ_BIT, .oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, .newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
         .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .image = tex, .subresourceRange = range };
      vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT, 0, 0, NULL, 0, NULL, 1, &to_read);
   }
   VkClearValue clear = { .color = { .float32 = { 0.10f, 0.12f, 0.16f, 1.0f } } };
   VkRenderPassBeginInfo rpbi = { .sType = VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO, .renderPass = rp, .framebuffer = fb, .renderArea = area, .clearValueCount = 1, .pClearValues = &clear };
   vkCmdBeginRenderPass(cb, &rpbi, VK_SUBPASS_CONTENTS_INLINE);
   vkCmdBindPipeline(cb, VK_PIPELINE_BIND_POINT_GRAPHICS, pipe);
   vkCmdBindDescriptorSets(cb, VK_PIPELINE_BIND_POINT_GRAPHICS, pl, 0, 1, &ds, 0, NULL);
   VkDeviceSize zero = 0; vkCmdBindVertexBuffers(cb, 0, 1, &vb, &zero);
   for (int i = 0; i < nquads; i++) {
      vkCmdPushConstants(cb, pl, VK_SHADER_STAGE_FRAGMENT_BIT, 0, 16, tints[i]);
      vkCmdDraw(cb, 6, 1, i * 6, 0);
   }
   vkCmdEndRenderPass(cb);
   CK(vkEndCommandBuffer(cb));
   VkFenceCreateInfo fci = { .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO }; VkFence fence; CK(vkCreateFence(dev, &fci, NULL, &fence));
   VkSubmitInfo si = { .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO, .commandBufferCount = 1, .pCommandBuffers = &cb };
   CK(vkQueueSubmit(q, 1, &si, fence));
   VkResult wr; wr = vkWaitForFences(dev, 1, &fence, VK_TRUE, 1000000000ull);
   printf("frame %d: fence wait -> %d\n", frame, wr);
   }
   VkResult wr = VK_SUCCESS;
   printf("submitted %d quad(s); fence wait -> %d\n", nquads, wr);

   char path[1024]; snprintf(path, sizeof(path), "%s/scene.txt", out); FILE *sf = fopen(path, "w");
   if (sf) {
      fprintf(sf, "dst %ux%u B8G8R8A8_UNORM %s rowPitch=%llu\n", W, H, dst_linear ? "linear" : "optimal", (unsigned long long)dl.rowPitch);
      fprintf(sf, "tex %ux%u R8G8B8A8_UNORM %s rowPitch=%llu\n", TW, TH, tex_linear ? "linear" : "optimal(staged)", (unsigned long long)tl.rowPitch);
      fprintf(sf, "%s 0.10 0.12 0.16 1.0\nblend src_alpha one_minus_src_alpha; alpha one zero\nrender area %d %d %u %u\n", load ? "load" : "clear", area.offset.x, area.offset.y, area.extent.width, area.extent.height);
      for (int i = 0; i < nquads; i++)
         fprintf(sf, "quad %d rect %g %g 512 512 tint %g %g %g %g\n", i, rects[i][0], rects[i][1], tints[i][0], tints[i][1], tints[i][2], tints[i][3]);
      fprintf(sf, "vertex stride 16: f32 x_px, y_px, u, v; 6 per quad\n");
      fclose(sf);
   }
   return 0;
}
