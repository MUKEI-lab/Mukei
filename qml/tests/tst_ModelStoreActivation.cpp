#include <QtQuickTest/quicktest.h>

#ifndef MODEL_STORE_ACTIVATION_QML_SOURCE_DIR
#error MODEL_STORE_ACTIVATION_QML_SOURCE_DIR must be defined
#endif

int main(int argc, char **argv) {
    return quick_test_main(argc, argv, "ModelStoreActivation", MODEL_STORE_ACTIVATION_QML_SOURCE_DIR);
}
