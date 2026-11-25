/* vk-probe: does Vulkan work with nothing but this package? Creates an instance, lists the GPUs
 * the installed drivers offer and, for each, opens a device with a compute queue.
 * Output (one line each): "vulkan: instance ok, N device(s)", then per device
 * "device: <name>, Vulkan a.b.c, driver <name>, compute queue <ok|missing|failed>".
 * Exit status 0 if the instance could be created. */
#include <stdio.h>
#include <string.h>
#include <vulkan/vulkan.h>

static const char *open_compute(VkPhysicalDevice gpu) {
    VkQueueFamilyProperties fam[16];
    uint32_t n = 16;
    vkGetPhysicalDeviceQueueFamilyProperties(gpu, &n, fam);
    for (uint32_t i = 0; i < n; i++) {
        if (!(fam[i].queueFlags & VK_QUEUE_COMPUTE_BIT)) continue;
        float prio = 1.0f;
        VkDeviceQueueCreateInfo q = {.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO, .queueFamilyIndex = i, .queueCount = 1, .pQueuePriorities = &prio};
        VkDeviceCreateInfo d = {.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, .queueCreateInfoCount = 1, .pQueueCreateInfos = &q};
        VkDevice dev;
        if (vkCreateDevice(gpu, &d, NULL, &dev) != VK_SUCCESS) return "failed";
        vkDestroyDevice(dev, NULL);
        return "ok";
    }
    return "missing";
}

int main(void) {
    VkApplicationInfo app = {.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO, .pApplicationName = "vk-probe", .apiVersion = VK_API_VERSION_1_1};
    VkInstanceCreateInfo ci = {.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, .pApplicationInfo = &app};
    VkInstance inst;
    VkResult r = vkCreateInstance(&ci, NULL, &inst);
    if (r != VK_SUCCESS) {
        printf("vulkan: instance failed (%d)\n", r);
        return 1;
    }
    VkPhysicalDevice gpus[8];
    uint32_t n = 8;
    r = vkEnumeratePhysicalDevices(inst, &n, gpus);
    if (r != VK_SUCCESS && r != VK_INCOMPLETE) n = 0;
    printf("vulkan: instance ok, %u device(s)\n", n);
    for (uint32_t i = 0; i < n; i++) {
        VkPhysicalDeviceDriverProperties drv = {.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DRIVER_PROPERTIES};
        VkPhysicalDeviceProperties2 p = {.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2, .pNext = &drv};
        vkGetPhysicalDeviceProperties2(gpus[i], &p);
        uint32_t v = p.properties.apiVersion;
        printf("device: %s, Vulkan %u.%u.%u, driver %s, compute queue %s\n", p.properties.deviceName, VK_VERSION_MAJOR(v), VK_VERSION_MINOR(v), VK_VERSION_PATCH(v), drv.driverName, open_compute(gpus[i]));
    }
    vkDestroyInstance(inst, NULL);
    return 0;
}
