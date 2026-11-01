//! Uma janela só. Enquanto o app abre, clicar de novo no atalho abria outra cópia (e outra, e outra), cada uma
//! brigando pelo cabo no modo exclusivo. Agora a segunda cópia só avisa a primeira, que vem para a frente, e sai.

/// O que fazer depois de olhar se já existe um GuitarForge aberto.
pub enum Claim {
    /// Este é o primeiro: segue abrindo. `None` se o Windows não deixou criar a marca (abre mesmo assim).
    First(Option<Instance>),
    /// Já havia um aberto, que foi avisado para aparecer.
    Second,
}

#[cfg(windows)]
pub use imp::{Instance, claim};

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForSingleObject};
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    use windows::core::w;

    use super::Claim;

    /// Marca de "já estou aberto" e o sinal que os próximos usam para chamar esta janela.
    pub struct Instance {
        _mutex: Handle,
        show: Handle,
    }

    struct Handle(HANDLE);
    // SAFETY: um HANDLE de mutex ou evento do Windows pode ser usado de qualquer thread.
    unsafe impl Send for Handle {}

    pub fn claim() -> Claim {
        // SAFETY: chamadas simples do Win32 com nomes fixos; os handles ficam abertos até o processo acabar.
        unsafe {
            let Ok(mutex) = CreateMutexW(None, false, w!("Local\\GuitarForge.instancia")) else {
                return Claim::First(None);
            };
            let already = GetLastError() == ERROR_ALREADY_EXISTS;
            let Ok(show) = CreateEventW(None, false, false, w!("Local\\GuitarForge.mostrar")) else {
                return Claim::First(None);
            };
            if already {
                // quem o usuário acabou de abrir pode passar o direito de vir para a frente ao outro processo
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let _ = SetEvent(show);
                return Claim::Second;
            }
            Claim::First(Some(Instance { _mutex: Handle(mutex), show: Handle(show) }))
        }
    }

    impl Instance {
        /// Fica esperando o aviso de outra cópia e chama `bring` a cada um (de outra thread).
        pub fn watch(self, bring: impl Fn() + Send + 'static) {
            let _ = std::thread::Builder::new().name("instancia".into()).spawn(move || {
                let me = self;
                // SAFETY: o evento é deste processo e vive tanto quanto `me`.
                while unsafe { WaitForSingleObject(me.show.0, INFINITE) } == WAIT_OBJECT_0 {
                    bring();
                }
            });
        }
    }
}

#[cfg(not(windows))]
pub struct Instance;

#[cfg(not(windows))]
impl Instance {
    pub fn watch(self, _bring: impl Fn() + Send + 'static) {}
}

#[cfg(not(windows))]
pub fn claim() -> Claim {
    Claim::First(None)
}
