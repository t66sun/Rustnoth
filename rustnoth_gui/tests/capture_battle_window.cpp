// From an x64 Visual Studio developer prompt with a Windows 10/11 SDK:
// cl /EHsc /std:c++20 capture_battle_window.cpp /link windowsapp.lib d3d11.lib dwmapi.lib user32.lib
// capture_battle_window.exe <game HWND decimal> <game PID> <output.rgb> <seconds>
// Captures the client of this game process. Keep the window size constant during sampling.
#include <windows.h>
#include <d3d11.h>
#include <dwmapi.h>
#include <windows.graphics.capture.interop.h>
#include <windows.graphics.directx.direct3d11.interop.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/Windows.Graphics.DirectX.Direct3D11.h>
#include <chrono>
#include <fstream>
#include <filesystem>
#include <iostream>
#include <vector>
#include <thread>
using namespace winrt;
using namespace winrt::Windows::Graphics::Capture;
using namespace winrt::Windows::Graphics::DirectX;
using namespace winrt::Windows::Graphics::DirectX::Direct3D11;
int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 5) return 2;
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        HWND h = reinterpret_cast<HWND>(std::stoull(argv[1]));
        DWORD pid = 0; GetWindowThreadProcessId(h, &pid);
        wchar_t name[255] = {};
        if (pid != std::stoul(argv[2]) || !GetClassNameW(h, name, 255) || wcscmp(name, L"MINIQUADAPP")) return 3;
        std::filesystem::path out(argv[3]);
        int seconds = std::stoi(argv[4]);
        init_apartment(apartment_type::multi_threaded);
        com_ptr<ID3D11Device> device;
        com_ptr<ID3D11DeviceContext> dc;
        check_hresult(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr, 0, D3D11_SDK_VERSION, device.put(), nullptr, dc.put()));
        auto dxgi = device.as<IDXGIDevice>();
        com_ptr<::IInspectable> direct;
        check_hresult(CreateDirect3D11DeviceFromDXGIDevice(dxgi.get(), direct.put()));
        auto interop = get_activation_factory<GraphicsCaptureItem,IGraphicsCaptureItemInterop>();
        GraphicsCaptureItem item{nullptr};
        check_hresult(interop->CreateForWindow(h, guid_of<GraphicsCaptureItem>(),put_abi(item)));
        auto pool = Direct3D11CaptureFramePool::CreateFreeThreaded(direct.as<IDirect3DDevice>(),
            DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, item.Size());
        auto session = pool.CreateCaptureSession(item);
        session.IsCursorCaptureEnabled(false);
        RECT r{}, b{}; POINT pt{};
        GetClientRect(h,&r);ClientToScreen(h,&pt);
        check_hresult(DwmGetWindowAttribute(h,DWMWA_EXTENDED_FRAME_BOUNDS,&b,sizeof(b)));
        auto size = item.Size();
        std::cerr<<"texture "<<size.Width<<"x"<<size.Height<<" client "<<r.right<<"x"<<r.bottom<<" offset "<<pt.x-b.left<<","<<pt.y-b.top<<"\n";
        int ox=pt.x-b.left, oy=pt.y-b.top;
        auto file=std::ofstream(out,std::ios::binary);
        int width=300, height=205;
        file.write(reinterpret_cast<char*>(&width),4);file.write(reinterpret_cast<char*>(&height),4);
        session.StartCapture();
        auto start=std::chrono::steady_clock::now();
        com_ptr<ID3D11Texture2D> stage;
        std::vector<uint8_t> rgb(width*height*3);
        int frames=0;
        while (std::chrono::steady_clock::now()-start<std::chrono::seconds(seconds)) {
            auto frame = pool.TryGetNextFrame();
            if (!frame) { std::this_thread::sleep_for(std::chrono::milliseconds(2)); continue; }
            auto dx=frame.Surface().as<::Windows::Graphics::DirectX::Direct3D11::IDirect3DDxgiInterfaceAccess>();
            com_ptr<ID3D11Texture2D> tex;
            check_hresult(dx->GetInterface(__uuidof(ID3D11Texture2D),tex.put_void()));
            if (!stage) {
                D3D11_TEXTURE2D_DESC d{};tex->GetDesc(&d);
                if (ox<0 || oy<0 || ox+r.right>int(d.Width) || oy+r.bottom>int(d.Height)) {
                    std::cerr<<"unmatched capture coordinates\n"; return 5;
                }
                d.Usage=D3D11_USAGE_STAGING;d.BindFlags=0;d.CPUAccessFlags=D3D11_CPU_ACCESS_READ;d.MiscFlags=0;
                check_hresult(device->CreateTexture2D(&d,nullptr,stage.put()));
            }
            dc->CopyResource(stage.get(),tex.get());
            D3D11_MAPPED_SUBRESOURCE map{};
            check_hresult(dc->Map(stage.get(),0,D3D11_MAP_READ,0,&map));
            for(int y=0;y<height;y++) {
                auto row=(uint8_t*)map.pData+(oy+(2*y+1)*r.bottom/(2*height))*map.RowPitch;
                for(int x=0;x<width;x++) {
                    auto p=row+(ox+(2*x+1)*r.right/(2*width))*4;
                    auto dst=&rgb[(y*width+x)*3];dst[0]=p[2];dst[1]=p[1];dst[2]=p[0];
                }
            }
            dc->Unmap(stage.get(),0);
            auto ticks=frame.SystemRelativeTime().count();
            file.write(reinterpret_cast<char*>(&ticks),8);
            file.write(reinterpret_cast<const char*>(rgb.data()),rgb.size());
            frames++;
        }
        session.Close();pool.Close();
        std::cerr<<frames<<" presented frames\n";
        return file && frames>0?0:4;
    } catch(hresult_error const& err) { std::wcerr<<std::hex<<err.code().value<<L" "<<err.message().c_str()<<L"\n";return 1; }
}
