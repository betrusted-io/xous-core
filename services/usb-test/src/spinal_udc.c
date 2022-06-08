//#define DEBUG

// cd $SAXON_ROOT/buildroot-build && make linux-rebuild && HOST_DIR=$SAXON_ROOT/buildroot-build/host BINARIES_DIR=$SAXON_ROOT/buildroot-build/images TARGET_DIR=$SAXON_ROOT/buildroot-build/target $SAXON_ROOT/buildroot-spinal-saxon/boards/common/post_build.sh && saxon_fpga_load
// https://aniembedded.com/2011/11/15/linux-usb-tests-using-gadget-zero-driver/
//https://mjmwired.net/kernel/Documentation/usb/gadget_configfs.rst

/*
echo "3" > /proc/sys/kernel/printk
./gadget.sh
sudo systemctl stop ModemManager.service
sudo systemctl disable ModemManager.service
#!/bin/sh
export CONFIGFS_HOME="/root/usb"
export GADGET_BASE_DIR="${CONFIGFS_HOME}/usb_gadget/g1"
export DEV_ETH_ADDR="aa:bb:cc:dd:ee:f1"
export HOST_ETH_ADDR="aa:bb:cc:dd:ee:f2"
#export USBDISK="/usbdisk.img"
export USBDISK="/dev/sda2"
# Create directory structure
mkdir -p "${CONFIGFS_HOME}/usb_gadget"
mount none $CONFIGFS_HOME -t configfs
mkdir -p "${GADGET_BASE_DIR}"
cd "${GADGET_BASE_DIR}"
mkdir -p configs/c.1/strings/0x409
mkdir -p strings/0x409
# Serial device
###
mkdir functions/acm.usb0
ln -s functions/acm.usb0 configs/c.1/
###
# Ethernet device
###
mkdir functions/ecm.usb0
echo "${DEV_ETH_ADDR}" > functions/ecm.usb0/dev_addr
echo "${HOST_ETH_ADDR}" > functions/ecm.usb0/host_addr
ln -s functions/ecm.usb0 configs/c.1/
###
# Mass Storage device
###
mkdir functions/mass_storage.usb0
echo 1 > functions/mass_storage.usb0/stall
echo 0 > functions/mass_storage.usb0/lun.0/cdrom
echo 0 > functions/mass_storage.usb0/lun.0/ro
echo 0 > functions/mass_storage.usb0/lun.0/nofua
echo "${USBDISK}" > functions/mass_storage.usb0/lun.0/file
ln -s functions/mass_storage.usb0 configs/c.1/
###
# Activate gadgets
echo 100b0000.udc > UDC
 */

#include <linux/delay.h>
#include <linux/device.h>
#include <linux/dma-mapping.h>
#include <linux/interrupt.h>
#include <linux/io.h>
#include <linux/module.h>
#include <linux/of_address.h>
#include <linux/of_device.h>
#include <linux/of_platform.h>
#include <linux/of_irq.h>
#include <linux/prefetch.h>
#include <linux/usb/ch9.h>
#include <linux/usb/gadget.h>

//todo short_not_ok


static const char driver_name[] = "spinal-udc";
static const char ep0name[] = "ep0";
#define EP0_MAX_PACKET       64
#define EP_MAX_PACKET       512
#define EPNAME_SIZE     4
#define DESC_HEADER_SIZE 12
#define DESC_SMALL_SIZE (64+4)
#define DESC_LARGE_SIZE (512+4)

#define DESC_LARGE_COUNT 4
#define EP_DESC_MAX 2

#define SPINAL_UDC_MAX_ENDPOINTS 16


#define USB_DEVICE_FRAME 0xFF00
#define USB_DEVICE_ADDRESS 0xFF04
#define USB_DEVICE_INTERRUPT 0xFF08
#define USB_DEVICE_HALT 0xFF0c
#define USB_DEVICE_CONFIG 0xFF10
#define USB_DEVICE_ADDRESS_WIDTH 0xFF20

#define USB_DEVICE_IRQ_RESET 16
#define USB_DEVICE_IRQ_SETUP 17
#define USB_DEVICE_IRQ_SUSPEND 18
#define USB_DEVICE_IRQ_RESUME 19
#define USB_DEVICE_IRQ_DISCONNECT 20

#define USB_DEVICE_CODE_NONE 0xF
#define USB_DEVICE_CODE_DONE 0x0

#define USB_DEVICE_CONFIG_PULLUP 0x1

#define USB_DEVICE_DESC_IN (1 << 16)
#define USB_DEVICE_DESC_OUT (0 << 16)
#define USB_DEVICE_DESC_SETUP (1 << 19)
#define USB_DEVICE_DESC_INTERRUPT (1 << 17)
#define USB_DEVICE_DESC_COMPL_ON_FULL (1 << 18)
#define USB_DEVICE_DESC_DATA1_COMPLETION (1 << 19)

#define USB_DEVICE_PULLUP_ENABLE  (1 << 0)
#define USB_DEVICE_PULLUP_DISABLE (2 << 0)
#define USB_DEVICE_INTERRUPT_ENABLE  (1 << 2)
#define USB_DEVICE_INTERRUPT_DISABLE (2 << 2)

#define USB_DEVICE_EP_ENABLE (1 << 0)
#define USB_DEVICE_EP_STALL (1 << 1)
#define USB_DEVICE_EP_NACK (1 << 2)
#define USB_DEVICE_EP_PHASE(x) (x << 3)
#define USB_DEVICE_EP_ISO (1 << 16)
#define USB_DEVICE_EP_MAX_PACKET_SIZE(x) (x << 22)

#define EP0_STATE_DATA 1
#define EP0_STATE_STATUS 2

#define to_udc(g)    container_of((g), struct spinal_udc, gadget)
#define to_spinal_udc_ep(ep)   container_of((ep), struct spinal_udc_ep, ep_usb)
#define to_spinal_udc_req(req) container_of((req), struct spinal_udc_req, usb_req)

struct spinal_udc_descriptor {
    struct list_head udc_node; //Used for both allocation queue and ep->descriptors
    struct list_head req_node;
    s32 address;
    u16 offset;
    u16 length_raw; //Number of data bytes allocated for the descriptor (not related to spinal_udc_req management)
    u16 length_deployed;
    void __iomem *mapping;
    bool req_completion;
    struct list_head *free;
};


struct spinal_udc_req {
    struct list_head ep_node;
    struct list_head descriptors;
    struct spinal_udc_ep *ep;
    u32 commited_length;
    u32 commited_once;
    struct usb_request usb_req;
};

struct spinal_udc_ep {
    struct usb_ep ep_usb;
    struct list_head reqs;
    struct spinal_udc *udc;
    const struct usb_endpoint_descriptor *desc;
    u32  rambase;
//    u32  offset;
    char name[EPNAME_SIZE];
    u16  epnumber;
    u16  maxpacket;
    bool is_in;
    bool is_iso;
    u32 pending_reqs_done;

    struct list_head descriptors;
    u32 descriptor_count;
};


struct spinal_udc {
    struct usb_gadget gadget;
    struct spinal_udc_ep *ep;
    struct usb_gadget_driver *driver;
    struct usb_ctrlrequest setup;
    struct device *dev;
    u32 usb_state;
    u32 remote_wkp;
    void __iomem *addr;
    spinlock_t lock;
    s32 ep_count;


    struct list_head dp_small;
    struct list_head dp_large;

    struct spinal_udc_descriptor ep0_setup;
    struct spinal_udc_req ep0_req;
    u8 ep0_req_data[64];
    u8 ep0_state;
    u16 refill_queue;
    u16 refill_robin;
    void            (*ep0_data_completion)(struct usb_ep *ep,  struct usb_request *req);
    struct spinal_udc_req *ep0_data_req;
};


/* Control endpoint configuration.*/
static const struct usb_endpoint_descriptor config_bulk_out_desc = {
    .bLength            = USB_DT_ENDPOINT_SIZE,
    .bDescriptorType    = USB_DT_ENDPOINT,
    .bEndpointAddress   = USB_DIR_OUT,
    .bmAttributes       = USB_ENDPOINT_XFER_BULK,
    .wMaxPacketSize     = EP0_MAX_PACKET,
};



static int __spinal_udc_ep0_queue(struct spinal_udc_ep *ep0, struct spinal_udc_req *req);
static void spinal_udc_ep_desc_refill(struct spinal_udc_ep *ep);
static void spinal_udc_nuke(struct spinal_udc_ep *ep, int status);
static void spinal_udc_ep_link_head(struct spinal_udc_ep *ep);


static void spinal_udc_hard_halt(struct spinal_udc_ep *ep){
    writel(ep->epnumber | 0x10, ep->udc->addr + USB_DEVICE_HALT);
    while ((readl(ep->udc->addr + USB_DEVICE_HALT) & 0x20) == 0) {}
}

static void spinal_udc_hard_unhalt(struct spinal_udc_ep *ep){
    writel(0, ep->udc->addr + USB_DEVICE_HALT);
}

static void spinal_udc_ep_status_mask(struct spinal_udc_ep *ep, u32 and, u32 or){
    u32 ep_status;
    spinal_udc_hard_halt(ep);
    ep_status = readl(ep->udc->addr + ep->epnumber * 4);
    writel((ep_status & and) | or, ep->udc->addr + ep->epnumber * 4);
    spinal_udc_hard_unhalt(ep);
}

//static void spinal_udc_ep_status_mask_no_halt(struct spinal_udc_ep *ep, u32 and, u32 or){
//    u32 ep_status;
//    ep_status = readl(ep->udc->addr + ep->epnumber * 4);
//    writel((ep_status & and) | or, ep->udc->addr + ep->epnumber * 4);
//}



static void spinal_udc_stop_activity(struct spinal_udc *udc){
    struct spinal_udc_ep *ep;
    int i;
    dev_dbg(udc->dev, "%s\n", __func__);

    for (i = 0; i < SPINAL_UDC_MAX_ENDPOINTS; i++) {
        ep = &udc->ep[i];
        spinal_udc_nuke(ep, -ESHUTDOWN);
    }
}

static void spinal_udc_clear_stall_all_ep(struct spinal_udc *udc){
    struct spinal_udc_ep *ep;
    int i;
    dev_dbg(udc->dev, "%s\n", __func__);

    for (i = 0; i < SPINAL_UDC_MAX_ENDPOINTS; i++) {
        ep = &udc->ep[i];
        spinal_udc_ep_status_mask(ep, ~(USB_DEVICE_EP_STALL | USB_DEVICE_EP_PHASE(1)), 0);
    }
}

static void spinal_udc_ep0_stall(struct spinal_udc *udc, bool throw_desc){
    dev_dbg(udc->dev, "%s 0\n", __func__);
    if(readl(udc->addr + USB_DEVICE_INTERRUPT) & USB_DEVICE_IRQ_SETUP){
        dev_dbg(udc->dev, "%s canceled\n", __func__);
        return;
    }
    spinal_udc_ep_status_mask(udc->ep, ~(throw_desc ? 0xFFF0 : 0), USB_DEVICE_EP_STALL);
    if(readl(udc->addr + USB_DEVICE_INTERRUPT) & USB_DEVICE_IRQ_SETUP){
        dev_dbg(udc->dev, "%s restoration\n", __func__);
        spinal_udc_ep_status_mask(udc->ep, ~USB_DEVICE_EP_STALL, 0);
        return;
    }
}

static void spinal_udc_ep_stall(struct spinal_udc *udc, struct spinal_udc_ep* ep, bool throw_desc){
    if(ep->epnumber == 0){
        spinal_udc_ep0_stall(udc, throw_desc);
        return;
    }
    dev_dbg(udc->dev, "%s %d\n", __func__, ep->epnumber);
    spinal_udc_ep_status_mask(ep, ~(throw_desc ? 0xFFF0 : 0), USB_DEVICE_EP_STALL);

}


static void spinal_udc_ep_unstall(struct spinal_udc *udc, struct spinal_udc_ep* ep, bool clear_phase){
    dev_dbg(udc->dev, "%s %d %d\n", __func__, ep->epnumber, clear_phase);
    spinal_udc_ep_status_mask(ep, ~(USB_DEVICE_EP_STALL | (clear_phase ? USB_DEVICE_EP_PHASE(1) : 0)), 0);
}


static void  spinal_udc_set_address_completion(struct usb_ep *_ep, struct usb_request *_req){
    struct spinal_udc_ep *ep = to_spinal_udc_ep(_ep);
    struct spinal_udc_req *req = to_spinal_udc_req(_req);
    struct spinal_udc *udc = ep->udc;
    dev_dbg(udc->dev, "%s\n", __func__);

    if(req->usb_req.status){
        dev_dbg(udc->dev, "%s bad status\n", __func__);
        writel(0, udc->addr + USB_DEVICE_ADDRESS);
        return;
    }

    dev_dbg(udc->dev, "%s %d\n", __func__, udc->setup.wValue);
}

static void spinal_udc_set_address(struct spinal_udc *udc){
    struct spinal_udc_ep *ep0 = &udc->ep[0];
    struct spinal_udc_req *req    = &udc->ep0_req;
    int ret;
    dev_dbg(udc->dev, "%s\n", __func__);

    req->usb_req.length = 0;
    req->usb_req.zero = 1;
    req->usb_req.short_not_ok = 1;
    req->usb_req.complete = spinal_udc_set_address_completion;
    writel(0x200 | udc->setup.wValue, udc->addr + USB_DEVICE_ADDRESS);

    ret = __spinal_udc_ep0_queue(ep0, req);

    if (ret == 0)
        return;

    dev_err(udc->dev, "Can't respond to SET ADDRESS request\n");
    spinal_udc_ep0_stall(udc, 1);
}

static void spinal_udc_ep_desc_free(struct spinal_udc_ep *ep, struct spinal_udc_descriptor *desc){
    struct spinal_udc *udc = ep->udc;
    list_move_tail(&desc->udc_node, desc->free);
    list_del(&desc->req_node);
    ep->descriptor_count -= 1;

    if(udc->refill_queue){
        s32 winner;
        if(udc->refill_queue & 1){
            winner = 0;
        } else {
            winner = udc->refill_robin;
            while(1){
                if(udc->refill_queue & (1 << winner)){
                    break;
                }
                winner += 1;
                winner &= 0xF;
            }
            udc->refill_robin = winner + 1;
        }
        spinal_udc_ep_desc_refill(udc->ep + winner);
    }
}

static void spinal_udc_done(struct spinal_udc_ep *ep, struct spinal_udc_req *req, int status)
{
    struct spinal_udc *udc = ep->udc;
    dev_dbg(udc->dev, "%s %d %d\n", __func__, ep->epnumber, req->usb_req.actual);

    list_del_init(&req->ep_node);

    if (req->usb_req.status == -EINPROGRESS)
        req->usb_req.status = status;
    else
        status = req->usb_req.status;

    if (status && status != -ESHUTDOWN)
        dev_dbg(udc->dev, "%s done %p, status %d\n",ep->ep_usb.name, req, status);

    if(!list_empty(&req->descriptors)){
        dev_dbg(udc->dev, "%s descriptors were not empty ! UNTESTED\n", __func__);
        spinal_udc_hard_halt(ep);
        while (!list_empty(&req->descriptors)) {
            struct spinal_udc_descriptor *desc, *entry;
            u32 next,tmp;
//
            desc = list_first_entry(&req->descriptors, struct spinal_udc_descriptor, req_node);
            entry = list_first_entry(&ep->descriptors, struct spinal_udc_descriptor, udc_node);
            next = readl(desc->mapping + 4) & 0xFFF0;
            if(entry == desc){ //On the hardware head
                tmp = readl(udc->addr + ep->epnumber*4) & ~0xFFF0;
                writel(tmp | next, udc->addr + ep->epnumber*4);
            } else { //in hardware tail
                entry = list_prev_entry(desc, udc_node);
                tmp = readl(entry->mapping + 4) & ~0xFFF0;
                writel(tmp | next, entry->mapping + 4);
            }

            spinal_udc_ep_desc_free(ep, desc);
        }
        spinal_udc_hard_unhalt(ep);
    }

    ep->pending_reqs_done -= 1;
    dev_dbg(udc->dev, "%s reqs done left %d %d\n", __func__, ep->epnumber, ep->pending_reqs_done);

    if (req->usb_req.complete) {
        spin_unlock(&udc->lock);
        dev_dbg(udc->dev, "%s complete call %x L=%d\n", __func__, (u32)req->usb_req.complete, req->usb_req.actual);
        req->usb_req.complete(&ep->ep_usb, &req->usb_req);
        spin_lock(&udc->lock);
    }
}

static void spinal_udc_nuke(struct spinal_udc_ep *ep, int status)
{
    struct spinal_udc_req *req;
    struct spinal_udc *udc = ep->udc;
    u32 tmp;
    dev_dbg(ep->udc->dev, "%s %d\n", __func__, ep->epnumber);
    tmp = readl(ep->udc->addr + ep->epnumber*4);
    dev_dbg(ep->udc->dev, "%s EP status : %x\n", __func__, tmp);
    tmp &= 0xFFF0;
    if(tmp){
        dev_dbg(ep->udc->dev, "%s DESC status : %x %x %x\n", __func__, readl(udc->addr + tmp + 0), readl(udc->addr + tmp + 4), readl(udc->addr + tmp + 8));
    }


    //Clear descriptors head
    spinal_udc_ep_status_mask(ep, ~0xFFF0, 0);

    while (!list_empty(&ep->reqs)) {
        req = list_first_entry(&ep->reqs, struct spinal_udc_req, ep_node);
        spinal_udc_done(ep, req, status);
    }
}



static void spinal_udc_get_status(struct spinal_udc* udc){
    struct spinal_udc_ep *ep0 = &udc->ep[0];
    struct spinal_udc_req *req    = &udc->ep0_req;
    struct spinal_udc_ep *target_ep;
    u16 status = 0;
    int epnum;
    u32 halt;
    int ret = 0;

    dev_dbg(udc->dev, "%s\n", __func__);

    switch (udc->setup.bRequestType & USB_RECIP_MASK) {
    case USB_RECIP_DEVICE:
        /* Get device status */
        status = 1 << USB_DEVICE_SELF_POWERED;
        if (udc->remote_wkp)
            status |= (1 << USB_DEVICE_REMOTE_WAKEUP);
        break;
    case USB_RECIP_INTERFACE:
        break;
    case USB_RECIP_ENDPOINT:
        epnum = udc->setup.wIndex & USB_ENDPOINT_NUMBER_MASK;
        target_ep = &udc->ep[epnum];
        halt = readl(udc->addr + target_ep->epnumber*4) & USB_DEVICE_EP_STALL;
        if(target_ep->epnumber) {
            if (udc->setup.wIndex & USB_DIR_IN) {
                if (!target_ep->is_in)
                    goto stall;
            } else {
                if (target_ep->is_in)
                    goto stall;
            }
        }
        if (halt)
            status = 1 << USB_ENDPOINT_HALT;
        break;
    default:
        goto stall;
    }

    req->usb_req.length = 2;
    req->usb_req.complete = NULL;
    *(u16 *)req->usb_req.buf = cpu_to_le16(status);

    ret = __spinal_udc_ep0_queue(ep0, req);
    if (ret == 0)
        return;
stall:
    dev_err(udc->dev, "Can't respond to getstatus request %d %d %d %d\n", udc->setup.bRequestType & USB_RECIP_MASK, udc->setup.wIndex & USB_DIR_IN, target_ep->is_in, ret);
    spinal_udc_ep0_stall(udc, 1);
}
static void spinal_udc_set_clear_feature(struct spinal_udc* udc){

    struct spinal_udc_ep *ep0 = &udc->ep[0];
    struct spinal_udc_req *req    = &udc->ep0_req;
    struct spinal_udc_ep *target_ep;
    u8 endpoint;
    u8 outinbit;
    int flag = (udc->setup.bRequest == USB_REQ_SET_FEATURE ? 1 : 0);
    int ret;
    u32 ep_status;

    dev_dbg(udc->dev, "%s\n", __func__);

    switch (udc->setup.bRequestType) {
    case USB_RECIP_DEVICE:
        switch (udc->setup.wValue) {
        case USB_DEVICE_TEST_MODE:
            /*
             * The Test Mode will be executed
             * after the status phase.
             */
            break;
        case USB_DEVICE_REMOTE_WAKEUP:
            if (flag)
                udc->remote_wkp = 1;
            else
                udc->remote_wkp = 0;
            break;
        default:
            spinal_udc_ep0_stall(udc, 1);
            break;
        }
        break;
    case USB_RECIP_ENDPOINT:
        if (!udc->setup.wValue) {
            endpoint = udc->setup.wIndex & USB_ENDPOINT_NUMBER_MASK;
            target_ep = &udc->ep[endpoint];
            outinbit = udc->setup.wIndex & USB_ENDPOINT_DIR_MASK;
            outinbit = outinbit >> 7;

            /* Make sure direction matches.*/
            if (outinbit != target_ep->is_in) {
                spinal_udc_ep0_stall(udc, 1);
                return;
            }
            ep_status = readl(udc->addr + target_ep->epnumber*4);
            if (!endpoint) {
                /* Clear the stall.*/
                spinal_udc_ep_unstall(udc, target_ep, 0);
            } else {
                if (flag) {
                    spinal_udc_ep_stall(udc, target_ep, 0);
                } else {
                    spinal_udc_ep_unstall(udc, target_ep, 1);
                }
            }
        }
        break;
    default:
        spinal_udc_ep0_stall(udc, 1);
        return;
    }

    req->usb_req.length = 0;
    req->usb_req.complete = NULL;
    ret = __spinal_udc_ep0_queue(ep0, req);
    if (ret == 0)
        return;

    dev_err(udc->dev, "Can't respond to SET/CLEAR FEATURE\n");
    spinal_udc_ep0_stall(udc, 1);
}

static void spinal_udc_ep0_status_completion(struct usb_ep *_ep, struct usb_request *_req){
    struct spinal_udc_ep *ep = to_spinal_udc_ep(_ep);
    struct spinal_udc *udc = ep->udc;
    struct usb_request *usb_data_req;
    dev_dbg(udc->dev, "%s\n", __func__);

    if(!udc->ep0_data_req)
        return;

    usb_data_req = &udc->ep0_data_req->usb_req;

    if (usb_data_req->complete && usb_data_req->complete != spinal_udc_ep0_status_completion) {
        dev_dbg(udc->dev, "%s complete call\n", __func__);
        usb_data_req->complete(_ep, usb_data_req);
    }
}

static void spinal_udc_ep0_status(struct spinal_udc* udc){
    struct spinal_udc_ep *ep = udc->ep;
    struct spinal_udc_req *req = &udc->ep0_req;
    int ret;

    ep->is_in = !ep->is_in;
    req->usb_req.length = 0;
    req->usb_req.zero = 1;
    req->usb_req.short_not_ok = 1;
    req->usb_req.complete = spinal_udc_ep0_status_completion;
    ret = __spinal_udc_ep0_queue(ep, req);

    if (ret == 0)
        return;

    dev_dbg(udc->dev, "%s error\n", __func__);
    spinal_udc_ep0_stall(udc, 1);
}

static void spinal_udc_ep0_data_completion(struct usb_ep *_ep, struct usb_request *req){
    struct spinal_udc_ep *ep = to_spinal_udc_ep(_ep);
    struct spinal_udc *udc = ep->udc;
    dev_dbg(udc->dev, "%s\n", __func__);

    req->complete = udc->ep0_data_completion;
    if(req->status){
        dev_dbg(udc->dev, "%s completion error\n", __func__);
        req->complete(_ep, req);
    } else {
        spinal_udc_ep0_status(udc);
    }
}

static void spinal_udc_setup_irq(struct spinal_udc *udc){
    struct spinal_udc_ep *ep0 = &udc->ep[0];
    int error;
    u32 payload[2];

    payload[0] = readl(udc->addr + 0x40 + 0);
    payload[1] = readl(udc->addr + 0x40 + 4);
    dev_dbg(udc->dev, "%s USB setup %08x %08x\n", __func__, cpu_to_be32(payload[0]), cpu_to_be32(payload[1]));
    memcpy(&udc->setup, payload, 8);
    spinal_udc_nuke(ep0, -ECONNRESET);
    udc->ep0_state = EP0_STATE_DATA;
    udc->ep0_data_req = NULL;

    if (udc->setup.bRequestType & USB_DIR_IN) {
        /* Execute the get command.*/
        ep0->is_in = 1;
    } else {
        /* Execute the put command.*/
        ep0->is_in = 0;
    }

    switch (udc->setup.bRequest) {
    case USB_REQ_GET_STATUS:
        /* Data+Status phase form udc */
        if ((udc->setup.bRequestType &
                (USB_DIR_IN | USB_TYPE_MASK)) !=
                (USB_DIR_IN | USB_TYPE_STANDARD))
            break;
        spinal_udc_get_status(udc);
        return;
    case USB_REQ_SET_ADDRESS:
        /* Status phase from udc */
        if (udc->setup.bRequestType != (USB_DIR_OUT |
                USB_TYPE_STANDARD | USB_RECIP_DEVICE))
            break;

        spinal_udc_set_address(udc);
        return;
    case USB_REQ_CLEAR_FEATURE:
    case USB_REQ_SET_FEATURE:
        /* Requests with no data phase, status phase from udc */
        if ((udc->setup.bRequestType & USB_TYPE_MASK) != USB_TYPE_STANDARD)
            break;
        spinal_udc_set_clear_feature(udc);
        return;
    default:
        break;
    }

    spin_unlock(&udc->lock);
    error = udc->driver->setup(&udc->gadget, &udc->setup);
    if (error < 0){
        dev_dbg(udc->dev, "%s driver unable to handle SETUP :( %d %x\n", __func__, error, (u32)udc->driver->setup);
        spinal_udc_ep0_stall(udc, 1);
    }
    spin_lock(&udc->lock);

}

static void spinal_udc_reset_irq(struct spinal_udc *udc){
    dev_dbg(udc->dev, "%s USB reset\n", __func__);
    udc->gadget.speed = USB_SPEED_FULL;

    spinal_udc_stop_activity(udc);
    spinal_udc_clear_stall_all_ep(udc);

    /* Set device address and remote wakeup to 0 */
    writel(0, udc->addr + USB_DEVICE_ADDRESS);
    udc->remote_wkp = 0;
    udc->usb_state = USB_STATE_DEFAULT;

    /* Enable the suspend, resume and disconnect */
//    intrreg = udc->read_fn(udc->addr + XUSB_IER_OFFSET);
//    intrreg |= XUSB_STATUS_SUSPEND_MASK | XUSB_STATUS_RESUME_MASK |
//           XUSB_STATUS_DISCONNECT_MASK;
//    udc->write_fn(udc->addr, XUSB_IER_OFFSET, intrreg);

    if (udc->driver && udc->driver->reset) {
        spin_unlock(&udc->lock);
        udc->driver->reset(&udc->gadget);
        spin_lock(&udc->lock);
    }
}


static void spinal_udc_suspend_irq(struct spinal_udc *udc){
    dev_dbg(udc->dev, "%s\n", __func__);

    if(udc->usb_state != USB_STATE_SUSPENDED && udc->usb_state != USB_STATE_NOTATTACHED){
        udc->usb_state = USB_STATE_SUSPENDED;
        if (udc->driver && udc->driver->suspend) {
            spin_unlock(&udc->lock);
            udc->driver->suspend(&udc->gadget);
            spin_lock(&udc->lock);
        }
    } else {
        dev_dbg(udc->dev, "%s ignored\n", __func__);
    }
}

static void spinal_udc_resume_irq(struct spinal_udc *udc){
    dev_dbg(udc->dev, "%s\n", __func__);
    udc->usb_state = 0;

    if (udc->driver && udc->driver->resume) {
        spin_unlock(&udc->lock);
        udc->driver->resume(&udc->gadget);
        spin_lock(&udc->lock);
    }
}

static void spinal_udc_disconnect_irq(struct spinal_udc *udc){
    dev_dbg(udc->dev, "%s\n", __func__);
    udc->usb_state = USB_STATE_NOTATTACHED;

    if (udc->driver && udc->driver->disconnect) {
        spin_unlock(&udc->lock);
        udc->driver->disconnect(&udc->gadget);
        spin_lock(&udc->lock);
    }
}




static void spinal_udc_ep_irq(struct spinal_udc_ep *ep){
    struct spinal_udc *udc = ep->udc;
    dev_dbg(udc->dev, "%s\n", __func__);
    while(1){
        struct spinal_udc_req *req;
        struct spinal_udc_descriptor *desc;
        req = list_first_entry_or_null(&ep->reqs, struct spinal_udc_req, ep_node);
        if(!req) return;
        while(1){
            u32 status, length;
            desc = list_first_entry_or_null(&req->descriptors, struct spinal_udc_descriptor, req_node);
            if(!desc) {
                //dev_dbg(udc->dev, "%s req without descriptors ?\n", __func__); //TODO refill there or somwere else
                return;
            }

            status = readl(desc->mapping + 0);
            if(((status >> 16) & 0xF) == USB_DEVICE_CODE_NONE) return;

            length = (status & 0xFFFF) - desc->offset;
//            dev_dbg(udc->dev, "%s %x %d\n", __func__, status, length);
            if(!ep->is_in){
                memcpy(req->usb_req.buf + req->usb_req.actual, desc->mapping + 12 + desc->offset, length);
            }

            req->usb_req.actual += length;

            spinal_udc_ep_desc_free(ep, desc);

            if(desc->req_completion || length != desc->length_deployed){
                req->usb_req.status = 0;
                spinal_udc_done(ep, req, 0);
                break;
            }
        }
    }
}

static void spinal_udc_epconfig(struct spinal_udc_ep *ep, struct spinal_udc *udc)
{
    writel(ep->ep_usb.maxpacket << 22, udc->addr + ep->epnumber*4);
}

static int spinal_udc_ep0_enable(struct usb_ep *ep,
               const struct usb_endpoint_descriptor *desc)
{
    return -EINVAL;
}

static int spinal_udc_ep0_disable(struct usb_ep *ep)
{
    return -EINVAL;
}

static irqreturn_t spinal_udc_irq(int irq, void *_udc)
{
    struct spinal_udc *udc = _udc;
    unsigned long flags;
    u32 pendings;

    spin_lock_irqsave(&udc->lock, flags);
    pendings = readl(udc->addr + USB_DEVICE_INTERRUPT);
//    dev_dbg(udc->dev, "%s pendings %x\n", __func__, pendings);
    writel(pendings, udc->addr + USB_DEVICE_INTERRUPT);

    while(pendings){
        int id = ffs(pendings)-1;
        if(id < 16){
            spinal_udc_ep_irq(udc->ep + id);
            spinal_udc_ep_desc_refill(udc->ep + id);
//            spinal_udc_ep_link_head(udc->ep + id);
        } else if(id == USB_DEVICE_IRQ_RESET) {
            spinal_udc_reset_irq(udc);
        } else if(id == USB_DEVICE_IRQ_SETUP) {
            spinal_udc_setup_irq(udc);
        } else if(id == USB_DEVICE_IRQ_SUSPEND) {
            spinal_udc_suspend_irq(udc);
        } else if(id == USB_DEVICE_IRQ_RESUME) {
            spinal_udc_resume_irq(udc);
        } else if(id == USB_DEVICE_IRQ_DISCONNECT) {
            spinal_udc_disconnect_irq(udc);
        } else {
            dev_dbg(udc->dev, "%s Unknown interrupt %d :(\n", __func__, id);
        }
//        dev_dbg(udc->dev, "%s ID %x\n", __func__, id);
        pendings &= ~(1 << id);
    }

    spin_unlock_irqrestore(&udc->lock, flags);
    return IRQ_HANDLED;

//    u32 intrstatus;
//    u32 ier;
//    u8 index;
//    u32 bufintr;
//    unsigned long flags;
//
//    spin_lock_irqsave(&udc->lock, flags);
//
//    /*
//     * Event interrupts are level sensitive hence first disable
//     * IER, read ISR and figure out active interrupts.
//     */
//    ier = udc->read_fn(udc->addr + XUSB_IER_OFFSET);
//    ier &= ~XUSB_STATUS_INTR_EVENT_MASK;
//    udc->write_fn(udc->addr, XUSB_IER_OFFSET, ier);
//
//    /* Read the Interrupt Status Register.*/
//    intrstatus = udc->read_fn(udc->addr + XUSB_STATUS_OFFSET);
//
//    /* Call the handler for the event interrupt.*/
//    if (intrstatus & XUSB_STATUS_INTR_EVENT_MASK) {
//        /*
//         * Check if there is any action to be done for :
//         * - USB Reset received {XUSB_STATUS_RESET_MASK}
//         * - USB Suspend received {XUSB_STATUS_SUSPEND_MASK}
//         * - USB Resume received {XUSB_STATUS_RESUME_MASK}
//         * - USB Disconnect received {XUSB_STATUS_DISCONNECT_MASK}
//         */
//        spinal_udc_startup_handler(udc, intrstatus);
//    }
//
//    /* Check the buffer completion interrupts */
//    if (intrstatus & XUSB_STATUS_INTR_BUFF_COMP_ALL_MASK) {
//        /* Enable Reset, Suspend, Resume and Disconnect  */
//        ier = udc->read_fn(udc->addr + XUSB_IER_OFFSET);
//        ier |= XUSB_STATUS_INTR_EVENT_MASK;
//        udc->write_fn(udc->addr, XUSB_IER_OFFSET, ier);
//
//        if (intrstatus & XUSB_STATUS_EP0_BUFF1_COMP_MASK)
//            spinal_udc_ctrl_ep_handler(udc, intrstatus);
//
//        for (index = 1; index < 8; index++) {
//            bufintr = ((intrstatus &
//                  (XUSB_STATUS_EP1_BUFF1_COMP_MASK <<
//                  (index - 1))) || (intrstatus &
//                  (XUSB_STATUS_EP1_BUFF2_COMP_MASK <<
//                  (index - 1))));
//            if (bufintr) {
//                spinal_udc_nonctrl_ep_handler(udc, index,
//                            intrstatus);
//            }
//        }
//    }
//
//    spin_unlock_irqrestore(&udc->lock, flags);

}


static int spinal_udc_get_frame(struct usb_gadget *gadget)
{
    struct spinal_udc *udc;
    int frame;

    if (!gadget)
        return -ENODEV;

    udc = to_udc(gadget);
    frame = readl(udc->addr + USB_DEVICE_FRAME);
    return frame;
}

static int spinal_udc_wakeup(struct usb_gadget *gadget)
{
    struct spinal_udc *udc = to_udc(gadget);
    dev_dbg(udc->dev, "%s call, UNIMPLMENTED\n", __func__);
    return 0;
}

static int spinal_udc_pullup(struct usb_gadget *gadget, int is_on)
{
    struct spinal_udc *udc = to_udc(gadget);
    unsigned long flags;
    dev_dbg(udc->dev, "%s call %d\n", __func__, is_on);
    spin_lock_irqsave(&udc->lock, flags);
    writel(is_on ? USB_DEVICE_PULLUP_ENABLE : USB_DEVICE_PULLUP_DISABLE, udc->addr + USB_DEVICE_CONFIG);
    spin_unlock_irqrestore(&udc->lock, flags);
    return 0;
}

//static void spinal_udc_ep_halt(struct spinal_udc_ep *ep){
//    writel(0x10 | ep->epnumber, ep->udc->addr + USB_DEVICE_HALT);
//    while(readl(ep->udc->addr + USB_DEVICE_HALT) & 0x20); //TODO
//}
//
//static void spinal_udc_ep_unhalt(struct spinal_udc_ep *ep){
//    writel(0, ep->udc->addr + USB_DEVICE_HALT);
//}


static int __spinal_udc_ep_enable(struct spinal_udc_ep *ep,
                const struct usb_endpoint_descriptor *desc)
{
    struct spinal_udc *udc = ep->udc;
    u32 tmp;
//    u32 epcfg;
//    u32 ier;
    u16 maxpacket;

    ep->is_in = ((desc->bEndpointAddress & USB_DIR_IN) != 0);
    /* Bit 3...0:endpoint number */
    ep->epnumber = (desc->bEndpointAddress & 0x0f);
    ep->desc = desc;
    ep->ep_usb.desc = desc;
    tmp = desc->bmAttributes & USB_ENDPOINT_XFERTYPE_MASK;
    ep->ep_usb.maxpacket = maxpacket = le16_to_cpu(desc->wMaxPacketSize);

    switch (tmp) {
    case USB_ENDPOINT_XFER_CONTROL:
        dev_dbg(udc->dev, "only one control endpoint\n");
        /* NON- ISO */
        ep->is_iso = 0;
        return -EINVAL;
    case USB_ENDPOINT_XFER_INT:
        /* NON- ISO */
        ep->is_iso = 0;
        if (maxpacket > 64) {
            dev_dbg(udc->dev, "bogus maxpacket %d\n", maxpacket);
            return -EINVAL;
        }
        break;
    case USB_ENDPOINT_XFER_BULK:
        /* NON- ISO */
        ep->is_iso = 0;
        if (!(is_power_of_2(maxpacket) && maxpacket >= 8 &&
                maxpacket <= 512)) {
            dev_dbg(udc->dev, "bogus maxpacket %d\n", maxpacket);
            return -EINVAL;
        }
        break;
    case USB_ENDPOINT_XFER_ISOC:
        /* ISO */
        ep->is_iso = 1;
        break;
    }

    spinal_udc_epconfig(ep, udc);

    dev_dbg(udc->dev, "Enable Endpoint %d max pkt is %d\n",
        ep->epnumber, maxpacket);

    /* Enable the End point.*/
    writel(USB_DEVICE_EP_ENABLE | USB_DEVICE_EP_PHASE(0) | USB_DEVICE_EP_MAX_PACKET_SIZE(maxpacket), udc->addr + ep->epnumber*4);

    dev_dbg(udc->dev, "%s done %d\n", __func__, ep->epnumber);
//    epcfg = udc->read_fn(udc->addr + ep->offset);
//    epcfg |= XUSB_EP_CFG_VALID_MASK;
//    udc->write_fn(udc->addr, ep->offset, epcfg);
//    if (ep->epnumber)
//        ep->rambase <<= 2;

    /* Enable buffer completion interrupts for endpoint */
//    ier = udc->read_fn(udc->addr + XUSB_IER_OFFSET);
//    ier |= (XUSB_STATUS_INTR_BUFF_COMP_SHIFT_MASK << ep->epnumber);
//    udc->write_fn(udc->addr, XUSB_IER_OFFSET, ier);

    /* for OUT endpoint set buffers ready to receive */ //TODO
//    if (ep->epnumber && !ep->is_in) {
//        udc->write_fn(udc->addr, XUSB_BUFFREADY_OFFSET,
//                  1 << ep->epnumber);
//        ep->buffer0ready = 1;
//        udc->write_fn(udc->addr, XUSB_BUFFREADY_OFFSET,
//                 (1 << (ep->epnumber +
//                  XUSB_STATUS_EP_BUFF2_SHIFT)));
//        ep->buffer1ready = 1;
//    }

    return 0;
}



/**
 * spinal_udc_ep_enable - Enables the given endpoint.
 * @_ep: pointer to the usb endpoint structure.
 * @desc: pointer to usb endpoint descriptor.
 *
 * Return: 0 for success and error value on failure
 */
static int spinal_udc_ep_enable(struct usb_ep *_ep,
              const struct usb_endpoint_descriptor *desc)
{
    struct spinal_udc *udc;
    struct spinal_udc_ep *ep;
    unsigned long flags;
    int ret;

    if (!_ep || !desc || desc->bDescriptorType != USB_DT_ENDPOINT) {
        pr_debug("%s: bad ep or descriptor\n", __func__);
        return -EINVAL;
    }

    ep = to_spinal_udc_ep(_ep);
    udc = ep->udc;

    dev_dbg(udc->dev, "%s call", __func__);

    if (!udc->driver || udc->gadget.speed == USB_SPEED_UNKNOWN) {
        dev_dbg(udc->dev, "bogus device state\n");
        return -ESHUTDOWN;
    }

    spin_lock_irqsave(&udc->lock, flags);
    ret = __spinal_udc_ep_enable(ep, desc);
    spin_unlock_irqrestore(&udc->lock, flags);

    return ret;
}





/**
 * spinal_udc_ep_disable - Disables the given endpoint.
 * @_ep: pointer to the usb endpoint structure.
 *
 * Return: 0 for success and error value on failure
 */
static int spinal_udc_ep_disable(struct usb_ep *_ep)
{
    struct spinal_udc_ep *ep;
    struct spinal_udc *udc;
    unsigned long flags;

    if (!_ep) {
        pr_debug("%s: invalid ep\n", __func__);
        return -EINVAL;
    }

    ep = to_spinal_udc_ep(_ep);
    udc = ep->udc;
    dev_dbg(udc->dev, "%s call", __func__);

    spin_lock_irqsave(&udc->lock, flags);

    spinal_udc_nuke(ep, -ESHUTDOWN);

    /* Restore the endpoint's pristine config */
    ep->desc = NULL;
    ep->ep_usb.desc = NULL;

    dev_dbg(udc->dev, "USB Ep %d disable\n ", ep->epnumber);

    /* Disable the endpoint.*/
    writel(0, udc->addr + ep->epnumber*4);

    spin_unlock_irqrestore(&udc->lock, flags);
    return 0;
}


static void spinal_udc_descriptor_push(struct spinal_udc *udc, struct spinal_udc_ep *ep, struct spinal_udc_descriptor* desc){

//    dev_dbg(udc->dev, "%s %d\n", __func__, ep->epnumber);
    if(!list_empty(&ep->descriptors)){
        struct spinal_udc_descriptor* last = list_last_entry(&ep->descriptors, struct spinal_udc_descriptor, udc_node);
        //dev_dbg(udc->dev, "%s push tail\n", __func__);
        writel(desc->address | ((last->length_deployed + last->offset) << 16), last->mapping + 4);
    } else {
        u32 status = readl(udc->addr + ep->epnumber*4);
        if((status & 0xFFF0) == 0) {
            //dev_dbg(udc->dev, "%s push head!\n", __func__);
            status = status & ~0xFFF0;
            status |= desc->address;
            writel(status, udc->addr + ep->epnumber*4);
        }
    }

    list_move_tail(&desc->udc_node, &ep->descriptors);
    ep->descriptor_count += 1;
//    printk("D%d %d", ep->epnumber, ep->descriptor_count);
}

static void spinal_udc_ep_link_head(struct spinal_udc_ep *ep){
    struct spinal_udc *udc = ep->udc;
    struct spinal_udc_descriptor* head;
    u32 status_ep, status_desc;

    dev_dbg(udc->dev, "%s\n", __func__);

    if(list_empty(&ep->descriptors)) return;
    head = list_first_entry(&ep->descriptors, struct spinal_udc_descriptor, udc_node);

    status_ep = readl(udc->addr + ep->epnumber*4);
    if((status_ep & 0xFFF0) != 0) {
        dev_dbg(udc->dev, "%s already linked\n", __func__);
        return;
    }

    status_desc = readl(head->mapping);
    if((status_desc & 0xF0000) != 0xF0000) {
        dev_dbg(udc->dev, "%s completion pending\n", __func__);
        return;
    }

    //dev_dbg(udc->dev, "%s push head!\n", __func__);
    status_ep = status_ep & ~0xFFF0;
    status_ep |= head->address;
    writel(status_ep, udc->addr + ep->epnumber*4);
}

static int spinal_udc_start(struct usb_gadget *gadget,
              struct usb_gadget_driver *driver)
{
    struct spinal_udc *udc = to_udc(gadget);
    struct spinal_udc_ep *ep0 = &udc->ep[0];
    const struct usb_endpoint_descriptor *desc = &config_bulk_out_desc;
    unsigned long flags;
    int ret = 0;
    dev_dbg(udc->dev, "%s\n", __func__);

    spin_lock_irqsave(&udc->lock, flags);

    if (udc->driver) {
        dev_err(udc->dev, "%s is already bound to %s\n",
            udc->gadget.name, udc->driver->driver.name);
        ret = -EBUSY;
        goto err;
    }

    /* hook up the driver */
    udc->driver = driver;
    udc->gadget.speed = driver->max_speed;

    /* Enable the control endpoint. */
    ret = __spinal_udc_ep_enable(ep0, desc); //__spinal_udc_ep_enable

    /* Set device address and remote wakeup to 0 */
    writel(0, udc->addr + USB_DEVICE_ADDRESS);

//    writel((USB_DEVICE_CODE_NONE << 16), udc->ep0_setup.mapping + 0);
//    writel((8 << 16), udc->ep0_setup.mapping + 4);
//    writel(USB_DEVICE_DESC_SETUP | USB_DEVICE_DESC_COMPL_ON_FULL | USB_DEVICE_DESC_INTERRUPT, udc->ep0_setup.mapping + 8);
//
//    spinal_udc_descriptor_push(udc, ep0, &udc->ep0_setup);
    udc->remote_wkp = 0;
err:
    spin_unlock_irqrestore(&udc->lock, flags);
    return ret;
}

/**
 * spinal_udc_stop - stops the device.
 * @gadget: pointer to the usb gadget structure
 *
 * Return: zero always
 */
static int spinal_udc_stop(struct usb_gadget *gadget)
{
    struct spinal_udc *udc = to_udc(gadget);
    unsigned long flags;
    dev_dbg(udc->dev, "%s call UNTESTED", __func__);

    spin_lock_irqsave(&udc->lock, flags);

    udc->gadget.speed = USB_SPEED_UNKNOWN;
    udc->driver = NULL;

    /* Set device address and remote wakeup to 0 */
    writel(0, udc->addr + USB_DEVICE_ADDRESS);
    udc->remote_wkp = 0;

    spinal_udc_stop_activity(udc);

    spin_unlock_irqrestore(&udc->lock, flags);

    return 0;
}







static void spinal_udc_req_init(struct spinal_udc_req *req, struct spinal_udc_ep *ep){
    INIT_LIST_HEAD(&req->ep_node);
    INIT_LIST_HEAD(&req->descriptors);
    req->ep = ep;
}


/**
 * spinal_udc_ep_alloc_request - Initializes the request queue.
 * @_ep: pointer to the usb endpoint structure.
 * @gfp_flags: Flags related to the request call.
 *
 * Return: pointer to request structure on success and a NULL on failure.
 */
static struct usb_request *spinal_udc_ep_alloc_request(struct usb_ep *_ep,
                         gfp_t gfp_flags)
{
    struct spinal_udc_ep *ep = to_spinal_udc_ep(_ep);
    struct spinal_udc *udc = ep->udc;
    struct spinal_udc_req *req;
    dev_dbg(udc->dev, "%s", __func__);

    req = kzalloc(sizeof(*req), gfp_flags);
    if (!req)
        return NULL;

    spinal_udc_req_init(req, ep);
    return &req->usb_req;
}

/**
 * spinal_udc_free_request - Releases the request from queue.
 * @_ep: pointer to the usb device endpoint structure.
 * @_req: pointer to the usb request structure.
 */
static void spinal_udc_ep_free_request(struct usb_ep *_ep, struct usb_request *_req)
{
    struct spinal_udc_ep *ep = to_spinal_udc_ep(_ep);
    struct spinal_udc *udc = ep->udc;
    struct spinal_udc_req *req = to_spinal_udc_req(_req);
    dev_dbg(udc->dev, "%s call", __func__);

    kfree(req);
}

//TODO WARNING, pid out shorter than max length will not discard subsequant descriptors.
static void spinal_udc_ep_desc_refill(struct spinal_udc_ep *ep){
    struct spinal_udc *udc = ep->udc;
    dev_dbg(udc->dev, "%s", __func__);
    spinal_udc_ep_link_head(ep);

    while(ep->descriptor_count != EP_DESC_MAX){
        u32 length, offset;
        u32 left;
        s32 word, word_count;
        u32 *src;
        u32 packet_end;
        void __iomem *dst;
        struct spinal_udc_req *req;
        struct spinal_udc_descriptor *desc;

        if (list_empty(&ep->reqs)) return;
        req = list_first_entry(&ep->reqs, struct spinal_udc_req, ep_node);
        left = req->usb_req.length - req->commited_length;
        if(left == 0 && req->commited_once) return;
//        if (!list_empty(&ep->descriptors)) return;
        dev_dbg(udc->dev, "dp_large %s %d", __func__, !list_empty(&udc->dp_large));

        if(left >= DESC_LARGE_SIZE-4 && !list_empty(&udc->dp_large)){
            dev_dbg(udc->dev, "%s dp_large picked", __func__);
            desc = list_first_entry(&udc->dp_large, struct spinal_udc_descriptor, udc_node);
//            printk("L %d %d\n", ep->epnumber, req->usb_req.length);
        } else if(!list_empty(&udc->dp_small) && (ep->epnumber == 0 || udc->dp_small.next != udc->dp_small.prev)){ //Only allow EP0 to take the last descriptor.
            desc = list_first_entry(&udc->dp_small, struct spinal_udc_descriptor, udc_node);
        } else {
            dev_dbg(udc->dev, "%s EMPTY !!!!", __func__);
            printk("spinal_udc running out of descriptor !!\n");
            if(ep->descriptor_count == 0){ //Will need some wakeup for a refill later on.
                udc->refill_queue |= 1 << ep->epnumber;
            }
            return;
        }

        length = min_t(u32, desc->length_raw, left);
        offset = ((u32)req->usb_req.buf + req->commited_length) & 0x3;
        desc->offset = offset;
        desc->req_completion = left == length;
        desc->length_deployed = length;
        packet_end = length == left && ep->is_in && !(ep->is_in && !req->usb_req.zero) && !(ep->epnumber == 0 && req->commited_length + length >= udc->setup.wLength);
        writel((USB_DEVICE_CODE_NONE << 16) | (offset), desc->mapping + 0);
        writel(((length + offset) << 16), desc->mapping + 4);
        writel(  (ep->is_in ? USB_DEVICE_DESC_IN : USB_DEVICE_DESC_OUT)
                | (packet_end ? 0 : USB_DEVICE_DESC_COMPL_ON_FULL)
                | (desc->req_completion && ep->epnumber == 0 ? USB_DEVICE_DESC_DATA1_COMPLETION : 0)
                | USB_DEVICE_DESC_INTERRUPT, desc->mapping + 8);

        if(ep->is_in){
            src = req->usb_req.buf + req->commited_length - offset;
            dst = desc->mapping + 12;
            word_count = (offset + length + 3)/4;
            for(word = 0;word < word_count;word++){
                writel(*src, dst);
//                if(req->usb_req.length >= 512) dev_dbg(udc->dev, "%s word %x %x", __func__, *src, (u32)dst);
                src+=1; dst+=4;
            }
        }

        list_add_tail(&desc->req_node, &req->descriptors);
        spinal_udc_descriptor_push(udc, ep, desc);
        req->commited_length += length;
        req->commited_once = 1;



        dev_dbg(udc->dev, "%s commited %d %d %d %d %d left %d z=%d s=%d\n", __func__,  ep->epnumber,  ep->is_in, length, desc->req_completion, packet_end, req->usb_req.length - req->commited_length, req->usb_req.zero , req->usb_req.short_not_ok);
    }
}

static int __spinal_udc_ep0_queue(struct spinal_udc_ep *ep0, struct spinal_udc_req *req)
{
    struct spinal_udc *udc = ep0->udc;
//    spinal_udc_ep_queue(ep0, req);
//    u32 length;
//    u8 *corebuf;
    ep0->pending_reqs_done += 1;
    dev_dbg(udc->dev, "%s %d pending %d", __func__, req->usb_req.length, ep0->pending_reqs_done);

    if (!udc->driver || udc->gadget.speed == USB_SPEED_UNKNOWN) {
        dev_dbg(udc->dev, "%s, bogus device state\n", __func__);
        return -EINVAL;
    }
    if (!list_empty(&ep0->reqs)) {
        dev_dbg(udc->dev, "%s:ep0 busy\n", __func__);
        return -EBUSY;
    }

    req->usb_req.status = -EINPROGRESS;
    req->usb_req.actual = 0;
    req->commited_length = 0;
    req->commited_once = 0;

    if(udc->ep0_state == EP0_STATE_DATA){
        udc->ep0_data_completion = req->usb_req.complete;
        udc->ep0_data_req = req;
        req->usb_req.complete = spinal_udc_ep0_data_completion;
        udc->ep0_state = EP0_STATE_STATUS;

        if(req->usb_req.length == 0){
            udc->ep0_data_req = NULL;
            req->usb_req.status = 0;
            ep0->pending_reqs_done -= 1;
            spinal_udc_ep0_data_completion(&ep0->ep_usb, &req->usb_req);
        } else {
            list_add_tail(&req->ep_node, &ep0->reqs);
        }
    } else {
        list_add_tail(&req->ep_node, &ep0->reqs);
    }


    spinal_udc_ep_desc_refill(ep0);

    return 0;
}



static int spinal_udc_ep0_queue(struct usb_ep *_ep, struct usb_request *_req,
              gfp_t gfp_flags)
{
    struct spinal_udc_req *req    = to_spinal_udc_req(_req);
    struct spinal_udc_ep  *ep0    = to_spinal_udc_ep(_ep);
    struct spinal_udc *udc    = ep0->udc;
    unsigned long flags;
    int ret;
    dev_dbg(udc->dev, "%s driver queue on EP0", __func__);
    spin_lock_irqsave(&udc->lock, flags);
    ret = __spinal_udc_ep0_queue(ep0, req);
    spin_unlock_irqrestore(&udc->lock, flags);

    return ret;
}

static int spinal_udc_ep_queue(struct usb_ep *_ep, struct usb_request *_req,
             gfp_t gfp_flags)
{
    struct spinal_udc_ep *ep = to_spinal_udc_ep(_ep);
    struct spinal_udc *udc = ep->udc;
    struct spinal_udc_req *req = to_spinal_udc_req(_req);
//    int  ret;
    unsigned long flags;
    dev_dbg(udc->dev, "%s %d %d", __func__, ep->epnumber, _req->length);
//
    if (!ep->desc) {
        dev_dbg(udc->dev, "%s: queuing request to disabled %s\n",
            __func__, ep->name);
        return -ESHUTDOWN;
    }

    if (!udc->driver || udc->gadget.speed == USB_SPEED_UNKNOWN) {
        dev_dbg(udc->dev, "%s, bogus device state\n", __func__);
        return -EINVAL;
    }

    spin_lock_irqsave(&udc->lock, flags);

//    if (list_empty(&ep->queue)) {
//        if (ep->is_in) {
//            dev_dbg(udc->dev, "spinal_udc_write_fifo from ep_queue\n");
//            if (!spinal_udc_write_fifo(ep, req))
//                req = NULL;
//        } else {
//            dev_dbg(udc->dev, "spinal_udc_read_fifo from ep_queue\n");
//            if (!spinal_udc_read_fifo(ep, req))
//                req = NULL;
//        }
//    }
//
//    if (req != NULL)
//        list_add_tail(&req->queue, &ep->queue);

    req->usb_req.status = -EINPROGRESS;
    req->usb_req.actual = 0;
    req->commited_length = 0;
    req->commited_once = 0;

    list_add_tail(&req->ep_node, &ep->reqs);
//#ifdef DEBUG
//    for(idx = 0;idx < req->usb_req.length;idx++){
//        printk("%02x ",((u8*)req->usb_req.buf)[idx]);
//    }
//    printk("\n");
//#endif
    ep->pending_reqs_done += 1;
    spinal_udc_ep_desc_refill(ep);

    spin_unlock_irqrestore(&udc->lock, flags);
    return 0;
}

/**
 * spinal_udc_ep_dequeue - Removes the request from the queue.
 * @_ep: pointer to the usb device endpoint structure.
 * @_req: pointer to the usb request structure.
 *
 * Return: 0 for success and error value on failure
 */
static int spinal_udc_ep_dequeue(struct usb_ep *_ep, struct usb_request *_req)
{
    struct spinal_udc_ep *ep = to_spinal_udc_ep(_ep);
    struct spinal_udc_req *req = to_spinal_udc_req(_req);
    struct spinal_udc *udc = ep->udc;
    unsigned long flags;
    dev_dbg(udc->dev, "%s call, UNTESTED %d", __func__, ep->epnumber);

    spin_lock_irqsave(&udc->lock, flags);
    /* Make sure it's actually queued on this endpoint */
    list_for_each_entry(req, &ep->reqs, ep_node) {
        if (&req->usb_req == _req)
            break;
    }
    if (&req->usb_req != _req) {
        spin_unlock_irqrestore(&udc->lock, flags);
        dev_dbg(udc->dev, "%s ????", __func__);
        return -EINVAL;
    }
    spinal_udc_done(ep, req, -ECONNRESET);
    spinal_udc_ep_desc_refill(ep);
    spin_unlock_irqrestore(&udc->lock, flags);

    return 0;
}

static int spinal_udc_ep_set_halt(struct usb_ep *_ep, int value)
{
    struct spinal_udc_ep *ep = to_spinal_udc_ep(_ep);
    struct spinal_udc *udc;
    unsigned long flags;
//
    if (!_ep || (!ep->desc && ep->epnumber)) {
        pr_debug("%s: bad ep or descriptor\n", __func__);
        return -EINVAL;
    }
    udc = ep->udc;

    dev_dbg(udc->dev, "%s call", __func__);

    if (ep->is_in && (!list_empty(&ep->reqs)) && value) {
        dev_dbg(udc->dev, "requests pending can't halt\n");
        return -EAGAIN;
    }
//
//    if (ep->buffer0ready || ep->buffer1ready) {
//        dev_dbg(udc->dev, "HW buffers busy can't halt\n");
//        return -EAGAIN;
//    }

    spin_lock_irqsave(&udc->lock, flags);

    if (value) {
        spinal_udc_ep_stall(udc, ep, 0);
    } else {
        spinal_udc_ep_unstall(udc, ep, ep->epnumber);
    }

    spin_unlock_irqrestore(&udc->lock, flags);
    return 0;
}

static const struct usb_ep_ops spinal_udc_ep0_ops = {//TOOD
    .enable         = spinal_udc_ep0_enable,
    .disable        = spinal_udc_ep0_disable,
    .alloc_request  = spinal_udc_ep_alloc_request,
    .free_request   = spinal_udc_ep_free_request,
    .queue          = spinal_udc_ep0_queue,
    .dequeue        = spinal_udc_ep_dequeue,
    .set_halt       = spinal_udc_ep_set_halt,
};

static const struct usb_ep_ops spinal_udc_ep_ops = {
    .enable         = spinal_udc_ep_enable,
    .disable        = spinal_udc_ep_disable,
    .alloc_request  = spinal_udc_ep_alloc_request,
    .free_request   = spinal_udc_ep_free_request,
    .queue          = spinal_udc_ep_queue,
    .dequeue        = spinal_udc_ep_dequeue,
    .set_halt       = spinal_udc_ep_set_halt,
};

static const struct usb_gadget_ops spinal_udc_ops = {
    .get_frame  = spinal_udc_get_frame,
    .wakeup     = spinal_udc_wakeup,
    .pullup     = spinal_udc_pullup,
    .udc_start  = spinal_udc_start,
    .udc_stop   = spinal_udc_stop,
};





static int spinal_udc_eps_init(struct spinal_udc *udc)
{
    u32 ep_number;

    INIT_LIST_HEAD(&udc->gadget.ep_list);

    udc->ep_count = 16;
    udc->ep = devm_kzalloc(udc->dev, sizeof(struct spinal_udc_ep)*udc->ep_count, GFP_KERNEL);
    if (!udc->ep)
        return -ENOMEM;

    for (ep_number = 0; ep_number < udc->ep_count; ep_number++) {
        struct spinal_udc_ep *ep = &udc->ep[ep_number];

        if (ep_number) {
            list_add_tail(&ep->ep_usb.ep_list, &udc->gadget.ep_list);
            usb_ep_set_maxpacket_limit(&ep->ep_usb, EP_MAX_PACKET); // (unsigned short) ~0
            snprintf(ep->name, EPNAME_SIZE, "ep%d", ep_number);
            ep->ep_usb.name = ep->name;
            ep->ep_usb.ops = &spinal_udc_ep_ops;

            ep->ep_usb.caps.type_iso = true;
            ep->ep_usb.caps.type_bulk = true;
            ep->ep_usb.caps.type_int = true;
        } else {
            ep->ep_usb.name = ep0name;
            usb_ep_set_maxpacket_limit(&ep->ep_usb, EP0_MAX_PACKET);
            ep->ep_usb.ops = &spinal_udc_ep0_ops;

            ep->ep_usb.caps.type_control = true;
        }

        ep->ep_usb.caps.dir_in = true;
        ep->ep_usb.caps.dir_out = true;

        ep->udc = udc;
        ep->epnumber = ep_number;
        ep->desc = NULL;
        ep->is_in = 0;
        ep->is_iso = 0;
        ep->maxpacket = 0;
        ep->pending_reqs_done = 0;
        spinal_udc_epconfig(ep, udc);

        INIT_LIST_HEAD(&ep->reqs);
        INIT_LIST_HEAD(&ep->descriptors);
        ep->descriptor_count = 0;
    }
    return 0;
}

static int spinal_udc_ram_init(struct spinal_udc *udc)
{
    s32 left = 1 << readl(udc->addr + USB_DEVICE_ADDRESS_WIDTH);
    s32 offset = 0;
    s32 tmp, idx;

    left   -= 0x40+8;
    offset += 0x40+8;

    for(tmp = 0; tmp < left; tmp += 4){
        writel(tmp + (u32)&left, udc->addr + offset + tmp);
    }


    udc->ep0_setup.address = offset;
    udc->ep0_setup.mapping = udc->addr + offset;
    left   -= DESC_HEADER_SIZE + 8; //TODO
    offset += DESC_HEADER_SIZE + 8;

    INIT_LIST_HEAD(&udc->dp_large);
    INIT_LIST_HEAD(&udc->dp_small);

    for(idx = 0;idx < DESC_LARGE_COUNT;idx++){
        struct spinal_udc_descriptor *desc;
        desc = devm_kzalloc(udc->dev, sizeof(*desc), GFP_KERNEL);
        if(!desc)
            return -ENOMEM;

        //Align
        tmp = (0x10 - (offset & 0xF)) & 0xF;
        left -= tmp;
        offset += tmp;

        desc->address = offset;
        desc->mapping = udc->addr + offset;
        desc->length_raw  = DESC_LARGE_SIZE - 4;
        desc->free = &udc->dp_large;
        left -= DESC_HEADER_SIZE + DESC_LARGE_SIZE;
        offset += DESC_HEADER_SIZE + DESC_LARGE_SIZE;
        list_add_tail(&desc->udc_node, &udc->dp_large);
        dev_dbg(udc->dev, "%s dp_large added", __func__);
    }


    if(left < 512){
        dev_dbg(udc->dev, "Not enough peripheral ram :(\n");
    }

    while(left >= DESC_HEADER_SIZE + DESC_SMALL_SIZE) {
        struct spinal_udc_descriptor *desc;
        desc = devm_kzalloc(udc->dev, sizeof(*desc), GFP_KERNEL);
        if(!desc)
            return -ENOMEM;

        //Align
        tmp = (0x10 - (offset & 0xF)) & 0xF;
        left -= tmp;
        offset += tmp;

        desc->address = offset;
        desc->mapping = udc->addr + offset;
        desc->length_raw  = DESC_SMALL_SIZE - 4;
        desc->free = &udc->dp_small;
        left -= DESC_HEADER_SIZE + DESC_SMALL_SIZE;
        offset += DESC_HEADER_SIZE + DESC_SMALL_SIZE;
        list_add_tail(&desc->udc_node, &udc->dp_small);
    }
    return 0;
}

static int spinal_udc_probe(struct platform_device *pdev)
{
//    struct device_node *np = pdev->dev.of_node;
    struct resource *res;
    struct spinal_udc *udc;
    int irq;
    int ret;
    int ed;
//    u32 ier;
//    u8 *buff;

    dev_dbg(&pdev->dev, "%s udc HI !!!", __func__);

    udc = devm_kzalloc(&pdev->dev, sizeof(*udc), GFP_KERNEL);
    if (!udc)
        return -ENOMEM;

    udc->dev = &pdev->dev;


    /* Create a dummy request for GET_STATUS, SET_ADDRESS */
//    udc->req = devm_kzalloc(&pdev->dev, sizeof(struct spinal_req), GFP_KERNEL);
//    if (!udc->req)
//        return -ENOMEM;
//
//    buff = devm_kzalloc(&pdev->dev, STATUSBUFF_SIZE, GFP_KERNEL);
//    if (!buff)
//        return -ENOMEM;
//
//    udc->req->usb_req.buf = buff;

    /* Map the registers */
    res = platform_get_resource(pdev, IORESOURCE_MEM, 0);
    udc->addr = devm_ioremap_resource(&pdev->dev, res);
    if (IS_ERR(udc->addr))
        return PTR_ERR(udc->addr);
    writel(USB_DEVICE_INTERRUPT_DISABLE || USB_DEVICE_PULLUP_DISABLE, udc->addr + USB_DEVICE_CONFIG);


    irq = platform_get_irq(pdev, 0);
    if (irq < 0)
        return irq;
    ret = devm_request_irq(&pdev->dev, irq, spinal_udc_irq, 0,
                   dev_name(&pdev->dev), udc);
    if (ret < 0) {
        dev_dbg(&pdev->dev, "unable to request irq %d", irq);
        goto fail;
    }

    if(spinal_udc_ram_init(udc))
        goto fail;


    spin_lock_init(&udc->lock);

    if(spinal_udc_eps_init(udc))
        goto fail;

    spinal_udc_req_init(&udc->ep0_req, &udc->ep[0]);
    udc->ep0_req.usb_req.buf = udc->ep0_req_data;
    udc->ep0_state = 0;
    udc->usb_state = USB_STATE_NOTATTACHED;

    /* Setup gadget structure */
    udc->gadget.ops = &spinal_udc_ops;
    udc->gadget.max_speed = USB_SPEED_FULL;
    udc->gadget.speed = USB_SPEED_UNKNOWN;
    udc->gadget.ep0 = &udc->ep[0].ep_usb;
    udc->gadget.name = driver_name;
    udc->refill_queue = 0;
    udc->refill_robin = 0;

//    /* Set device address to 0.*/
    writel(0, udc->addr + USB_DEVICE_ADDRESS);
    for(ed = 0;ed < 16; ed++){
        writel(0, udc->addr + ed*4);
    }
//
    ret = usb_add_gadget_udc(&pdev->dev, &udc->gadget);
    if (ret)
        goto fail;

//    dev_info(&pdev->dev, "%x VS %x\n", udc->dev, &udc->gadget.dev);
//    dev_info(&pdev->dev, "%x %x\n", udc->gadget.ep0, udc->ep);



    udc->dev = &udc->gadget.dev;
//
//    /* Enable the interrupts.*/
//    ier = XUSB_STATUS_GLOBAL_INTR_MASK | XUSB_STATUS_INTR_EVENT_MASK |
//          XUSB_STATUS_FIFO_BUFF_RDY_MASK | XUSB_STATUS_FIFO_BUFF_FREE_MASK |
//          XUSB_STATUS_SETUP_PACKET_MASK |
//          XUSB_STATUS_INTR_BUFF_COMP_ALL_MASK;
//
//    udc->write_fn(udc->addr, XUSB_IER_OFFSET, ier);
//

    writel(0xFFFFFFFF, udc->addr + USB_DEVICE_INTERRUPT);
    writel(USB_DEVICE_INTERRUPT_ENABLE, udc->addr + USB_DEVICE_CONFIG);

    platform_set_drvdata(pdev, udc);

    dev_info(&pdev->dev, "%s at 0x%08X mapped with irq %d\n",
        driver_name, (u32)res->start, irq);

    return 0;
fail:
    dev_err(&pdev->dev, "probe failed, %d\n", ret);
    return ret;
}


static int spinal_udc_remove(struct platform_device *pdev)
{
    struct spinal_udc *udc = platform_get_drvdata(pdev);

    usb_del_gadget_udc(&udc->gadget);

    return 0;
}

/* Match table for of_platform binding */
static const struct of_device_id usb_of_match[] = {
    { .compatible = "spinal-udc", },
    { /* end of list */ },
};
MODULE_DEVICE_TABLE(of, usb_of_match);

static struct platform_driver spinal_udc_driver = {
    .driver = {
        .name = driver_name,
        .of_match_table = usb_of_match,
    },
    .probe = spinal_udc_probe,
    .remove = spinal_udc_remove,
};

module_platform_driver(spinal_udc_driver);

MODULE_DESCRIPTION("SpinalHDL udc driver");
MODULE_AUTHOR("SpinalHDL");
MODULE_LICENSE("GPL");
