/* vk-rechnen: does the GPU compute through Vulkan with nothing but this package? A compute shader
 * fills a buffer of N values (v[i] = i*i + 7); the CPU checks every one of them.
 * Output: "device: <name>" and the verdict "VKRECHNEN: ALLES OK (...)" or "VKRECHNEN: FEHLER <step>".
 * Exit status 0 only for ALLES OK. The shader (rechnen.comp) is compiled to rechnen_spv.h when
 * the package check is built. */
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <vulkan/vulkan.h>
#include "rechnen_spv.h"

#define N 4096u
#define CHECK(step, call) do { VkResult r_ = (call); if (r_ != VK_SUCCESS) { printf("VKRECHNEN: FEHLER %s (%d)\n", step, r_); return 1; } } while (0)

static VkInstance inst;
static VkPhysicalDevice gpu;
static VkDevice dev;
static VkQueue queue;
static uint32_t family, mem_type;

static double now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

/* instance, first GPU with a compute queue, device */
static int open_device(void) {
    VkApplicationInfo app = {.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO, .pApplicationName = "vk-rechnen", .apiVersion = VK_API_VERSION_1_1};
    VkInstanceCreateInfo ci = {.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, .pApplicationInfo = &app};
    CHECK("instance", vkCreateInstance(&ci, NULL, &inst));
    uint32_t n = 1;
    VkResult r = vkEnumeratePhysicalDevices(inst, &n, &gpu);
    if ((r != VK_SUCCESS && r != VK_INCOMPLETE) || n == 0) { printf("VKRECHNEN: FEHLER no device\n"); return 1; }
    VkPhysicalDeviceProperties p;
    vkGetPhysicalDeviceProperties(gpu, &p);
    printf("device: %s\n", p.deviceName);
    VkQueueFamilyProperties fam[16];
    uint32_t nf = 16;
    vkGetPhysicalDeviceQueueFamilyProperties(gpu, &nf, fam);
    for (family = 0; family < nf && !(fam[family].queueFlags & VK_QUEUE_COMPUTE_BIT); family++) {}
    if (family == nf) { printf("VKRECHNEN: FEHLER no compute queue\n"); return 1; }
    float prio = 1.0f;
    VkDeviceQueueCreateInfo q = {.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO, .queueFamilyIndex = family, .queueCount = 1, .pQueuePriorities = &prio};
    VkDeviceCreateInfo d = {.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, .queueCreateInfoCount = 1, .pQueueCreateInfos = &q};
    CHECK("device", vkCreateDevice(gpu, &d, NULL, &dev));
    vkGetDeviceQueue(dev, family, 0, &queue);
    return 0;
}

/* a storage buffer the CPU can see */
static int make_buffer(VkBuffer *buf, VkDeviceMemory *mem, uint32_t **map) {
    VkBufferCreateInfo b = {.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO, .size = N * 4, .usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT, .sharingMode = VK_SHARING_MODE_EXCLUSIVE};
    CHECK("buffer", vkCreateBuffer(dev, &b, NULL, buf));
    VkMemoryRequirements req;
    vkGetBufferMemoryRequirements(dev, *buf, &req);
    VkPhysicalDeviceMemoryProperties mp;
    vkGetPhysicalDeviceMemoryProperties(gpu, &mp);
    const VkMemoryPropertyFlags want = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    for (mem_type = 0; mem_type < mp.memoryTypeCount; mem_type++)
        if ((req.memoryTypeBits & (1u << mem_type)) && (mp.memoryTypes[mem_type].propertyFlags & want) == want) break;
    if (mem_type == mp.memoryTypeCount) { printf("VKRECHNEN: FEHLER no host-visible memory\n"); return 1; }
    VkMemoryAllocateInfo a = {.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, .allocationSize = req.size, .memoryTypeIndex = mem_type};
    CHECK("memory", vkAllocateMemory(dev, &a, NULL, mem));
    CHECK("bind memory", vkBindBufferMemory(dev, *buf, *mem, 0));
    CHECK("map", vkMapMemory(dev, *mem, 0, VK_WHOLE_SIZE, 0, (void **)map));
    memset(*map, 0xff, N * 4);
    return 0;
}

/* the compute pipeline and the descriptor set that names the buffer */
static int make_pipeline(VkBuffer buf, VkPipeline *pipe, VkPipelineLayout *layout, VkDescriptorSet *set) {
    VkDescriptorSetLayoutBinding bind = {.binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT};
    VkDescriptorSetLayoutCreateInfo dl = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO, .bindingCount = 1, .pBindings = &bind};
    VkDescriptorSetLayout dsl;
    CHECK("set layout", vkCreateDescriptorSetLayout(dev, &dl, NULL, &dsl));
    VkPipelineLayoutCreateInfo pl = {.sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO, .setLayoutCount = 1, .pSetLayouts = &dsl};
    CHECK("pipeline layout", vkCreatePipelineLayout(dev, &pl, NULL, layout));
    VkShaderModuleCreateInfo sm = {.sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO, .codeSize = sizeof(rechnen_spv), .pCode = rechnen_spv};
    VkShaderModule mod;
    CHECK("shader", vkCreateShaderModule(dev, &sm, NULL, &mod));
    VkComputePipelineCreateInfo cp = {.sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO, .layout = *layout,
        .stage = {.sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO, .stage = VK_SHADER_STAGE_COMPUTE_BIT, .module = mod, .pName = "main"}};
    CHECK("pipeline", vkCreateComputePipelines(dev, VK_NULL_HANDLE, 1, &cp, NULL, pipe));
    VkDescriptorPoolSize ps = {.type = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1};
    VkDescriptorPoolCreateInfo dp = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO, .maxSets = 1, .poolSizeCount = 1, .pPoolSizes = &ps};
    VkDescriptorPool pool;
    CHECK("descriptor pool", vkCreateDescriptorPool(dev, &dp, NULL, &pool));
    VkDescriptorSetAllocateInfo da = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO, .descriptorPool = pool, .descriptorSetCount = 1, .pSetLayouts = &dsl};
    CHECK("descriptor set", vkAllocateDescriptorSets(dev, &da, set));
    VkDescriptorBufferInfo bi = {.buffer = buf, .offset = 0, .range = VK_WHOLE_SIZE};
    VkWriteDescriptorSet w = {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstSet = *set, .dstBinding = 0, .descriptorCount = 1, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .pBufferInfo = &bi};
    vkUpdateDescriptorSets(dev, 1, &w, 0, NULL);
    return 0;
}

/* one dispatch over all values, waited for */
static int run(VkPipeline pipe, VkPipelineLayout layout, VkDescriptorSet set) {
    VkCommandPoolCreateInfo cpi = {.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO, .queueFamilyIndex = family};
    VkCommandPool pool;
    CHECK("command pool", vkCreateCommandPool(dev, &cpi, NULL, &pool));
    VkCommandBufferAllocateInfo ca = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO, .commandPool = pool, .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = 1};
    VkCommandBuffer cmd;
    CHECK("command buffer", vkAllocateCommandBuffers(dev, &ca, &cmd));
    VkCommandBufferBeginInfo bi = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO, .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT};
    CHECK("begin", vkBeginCommandBuffer(cmd, &bi));
    vkCmdBindPipeline(cmd, VK_PIPELINE_BIND_POINT_COMPUTE, pipe);
    vkCmdBindDescriptorSets(cmd, VK_PIPELINE_BIND_POINT_COMPUTE, layout, 0, 1, &set, 0, NULL);
    vkCmdDispatch(cmd, N / 64, 1, 1);
    CHECK("end", vkEndCommandBuffer(cmd));
    VkFenceCreateInfo fi = {.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
    VkFence fence;
    CHECK("fence", vkCreateFence(dev, &fi, NULL, &fence));
    VkSubmitInfo si = {.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO, .commandBufferCount = 1, .pCommandBuffers = &cmd};
    CHECK("submit", vkQueueSubmit(queue, 1, &si, fence));
    CHECK("wait", vkWaitForFences(dev, 1, &fence, VK_TRUE, 10ull * 1000 * 1000 * 1000));
    return 0;
}

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    double t0 = now_ms();
    if (open_device()) return 1;
    VkBuffer buf;
    VkDeviceMemory mem;
    uint32_t *v;
    if (make_buffer(&buf, &mem, &v)) return 1;
    VkPipeline pipe;
    VkPipelineLayout layout;
    VkDescriptorSet set;
    if (make_pipeline(buf, &pipe, &layout, &set)) return 1;
    double t1 = now_ms();
    if (run(pipe, layout, set)) return 1;
    double t2 = now_ms();
    uint32_t wrong = 0, first = 0;
    for (uint32_t i = 0; i < N; i++)
        if (v[i] != i * i + 7u) { if (!wrong) first = i; wrong++; }
    if (wrong) {
        printf("VKRECHNEN: FEHLER %u of %u values wrong, first at %u: 0x%08x instead of 0x%08x\n", wrong, N, first, v[first], first * first + 7u);
        return 1;
    }
    printf("VKRECHNEN: ALLES OK (%u values computed by the GPU, setup %.0f ms, job %.1f ms)\n", N, t1 - t0, t2 - t1);
    return 0;
}
