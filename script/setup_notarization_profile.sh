#!/bin/bash
# Skrypt do tworzenia profilu notaryzacji w pęku kluczy

echo "================================================================="
echo "           Tworzenie profilu notaryzacji w pęku kluczy"
echo "================================================================="
echo "Ten skrypt utworzy profil notaryzacji, który będzie przechowywany"
echo "w pęku kluczy macOS, dzięki czemu nie będziesz musiał podawać"
echo "danych uwierzytelniających przy każdym użyciu narzędzia notarytool."
echo "================================================================="
echo

# Sprawdź czy notarytool jest dostępny
if ! command -v xcrun notarytool &> /dev/null; then
    echo "Błąd: Narzędzie 'notarytool' nie jest zainstalowane."
    echo "Zainstaluj najnowszą wersję Xcode i narzędzi wiersza poleceń:"
    echo "xcode-select --install"
    exit 1
fi

# Zbierz informacje od użytkownika
echo "Podaj informacje potrzebne do utworzenia profilu:"
read -p "Nazwa profilu (np. 'notaryzacja'): " profile_name
read -p "Apple ID (email): " apple_id
read -p "Team ID (z konta Apple Developer): " team_id
echo
echo "Teraz potrzebujesz hasło aplikacji (nie zwykłe hasło do Apple ID)."
echo "Jeśli nie masz hasła aplikacji, utwórz je na stronie:"
echo "appleid.apple.com → Bezpieczeństwo → Hasła aplikacji"
echo

# Utwórz profil
echo "Tworzenie profilu '${profile_name}'..."
xcrun notarytool store-credentials "${profile_name}" \
    --apple-id "${apple_id}" \
    --team-id "${team_id}"

if [ $? -eq 0 ]; then
    echo
    echo "================================================================="
    echo "Profil został pomyślnie utworzony i zapisany w pęku kluczy macOS."
    echo "Aby użyć tego profilu w skrypcie notaryzacji, uruchom:"
    echo
    echo "./macos_service_package_builder.sh --keychain-profile ${profile_name}"
    echo
    echo "lub zmodyfikuj skrypt, aby używał zmiennej KEYCHAIN_PROFILE:"
    echo
    echo "KEYCHAIN_PROFILE=\"${profile_name}\""
    echo "================================================================="
else
    echo
    echo "Wystąpił błąd podczas tworzenia profilu. Upewnij się, że:"
    echo "1. Podałeś poprawne informacje (Apple ID, Team ID)"
    echo "2. Używasz hasła aplikacji, a nie zwykłego hasła do Apple ID"
    echo "3. Masz włączoną weryfikację dwuetapową na koncie Apple ID"
fi

exit 0
